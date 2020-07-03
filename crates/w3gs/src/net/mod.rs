use futures::ready;
use tokio::net::{TcpListener, TcpStream};
use tokio::stream::Stream;
use tokio_util::codec::Framed;

use crate::error::*;

mod codec;
use self::codec::W3GSCodec;
use crate::protocol::packet::Packet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug)]
pub struct W3GSListener {
  listener: TcpListener,
  local_addr: SocketAddr,
}

impl W3GSListener {
  pub async fn bind() -> Result<Self, Error> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
    let local_addr = listener.local_addr()?;
    Ok(W3GSListener {
      listener,
      local_addr,
    })
  }

  pub fn incoming(&mut self) -> Incoming {
    Incoming::new(&mut self.listener)
  }

  pub fn local_addr(&self) -> &SocketAddr {
    &self.local_addr
  }

  pub fn port(&self) -> u16 {
    self.local_addr.port()
  }
}

#[derive(Debug)]
pub struct W3GSStream {
  addr: SocketAddr,
  transport: Framed<TcpStream, W3GSCodec>,
}

impl W3GSStream {
  pub fn addr(&self) -> SocketAddr {
    self.addr
  }
}

impl Stream for W3GSStream {
  type Item = Result<Packet>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Pin::new(&mut self.transport).poll_next(cx)
  }
}

pub struct Incoming<'a> {
  inner: &'a mut TcpListener,
}

impl Incoming<'_> {
  pub(crate) fn new(listener: &mut TcpListener) -> Incoming<'_> {
    Incoming { inner: listener }
  }

  pub fn poll_accept(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<W3GSStream>> {
    let (socket, addr) = ready!(self.inner.poll_accept(cx))?;

    socket.set_nodelay(true).ok();
    socket.set_keepalive(None).ok();

    let stream = W3GSStream {
      addr,
      transport: Framed::new(socket, W3GSCodec::new()),
    };

    Poll::Ready(Ok(stream))
  }
}

impl Stream for Incoming<'_> {
  type Item = Result<W3GSStream>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let stream = ready!(self.poll_accept(cx))?;
    Poll::Ready(Some(Ok(stream)))
  }
}
