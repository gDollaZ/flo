mod cache;
mod consumer;
mod env;
pub mod error;
pub use flo_observer_fs as fs;

use crate::cache::Cache;
use crate::consumer::StartShardConsumer;
use crate::error::Error;
use consumer::ShardConsumer;
use error::Result;
use flo_observer::{KINESIS_CLIENT, KINESIS_STREAM_NAME};
use flo_state::{async_trait, Actor, Context, Handler, Message, Owner};
use rusoto_kinesis::Kinesis;
use std::collections::BTreeMap;

pub struct FloObserver;

impl FloObserver {
  pub async fn serve() -> Result<()> {
    let _actor = ShardsMgr::init().await?.start();
    std::future::pending::<()>().await;
    Ok(())
  }
}

#[derive(Debug)]
pub(crate) struct ShardsMgr {
  cache: Cache,
  shard_ids: Vec<String>,
  shards: BTreeMap<String, Owner<ShardConsumer>>,
}

impl ShardsMgr {
  async fn init() -> Result<Self> {
    use rusoto_kinesis::ListShardsInput;

    let cache = Cache::connect().await?;

    let shards = KINESIS_CLIENT
      .list_shards(ListShardsInput {
        stream_name: Some(KINESIS_STREAM_NAME.clone()),
        ..Default::default()
      })
      .await?;

    let shard_ids: Vec<_> = shards
      .shards
      .ok_or_else(|| Error::NoShards)?
      .into_iter()
      .map(|shard| shard.shard_id)
      .collect();
    tracing::info!("shards: {:?}", shard_ids);

    Ok(Self {
      cache,
      shard_ids,
      shards: Default::default(),
    })
  }

  async fn start_consumers(&mut self, ctx: &mut Context<Self>) -> Result<()> {
    let addr = ctx.addr();

    let shards: BTreeMap<_, _> = self
      .shard_ids
      .iter()
      .cloned()
      .map(|id| {
        let actor = ShardConsumer::new(id.clone(), addr.clone(), self.cache.clone()).start();
        (id, actor)
      })
      .collect();

    let game_ids = self.cache.list_games().await?;
    let mut shard_games = BTreeMap::new();
    for id in game_ids {
      if let Some(game) = self.cache.get_game_state(id).await? {
        shard_games
          .entry(game.shard_id.clone())
          .or_insert_with(|| vec![])
          .push(game);
      }
    }

    for (shard_id, actor) in &shards {
      let recovered_games = if let Some(recovered_games) = shard_games.remove(shard_id) {
        tracing::info!(
          "recovered shard games: {} = {}",
          shard_id,
          recovered_games.len()
        );
        recovered_games
      } else {
        vec![]
      };
      actor.send(StartShardConsumer { recovered_games }).await??;
    }

    self.shards = shards;

    Ok(())
  }
}

#[async_trait]
impl Actor for ShardsMgr {
  async fn started(&mut self, ctx: &mut Context<Self>) {
    if let Err(err) = self.start_consumers(ctx).await {
      tracing::error!("start consumers: {}", err);
    }
  }
}

struct RemoveShard {
  shard_id: String,
}

impl Message for RemoveShard {
  type Result = ();
}

#[async_trait]
impl Handler<RemoveShard> for ShardsMgr {
  async fn handle(&mut self, _ctx: &mut Context<Self>, RemoveShard { shard_id }: RemoveShard) {
    tracing::warn!("remove shard: {}", shard_id);
    self.shards.remove(&shard_id);
  }
}