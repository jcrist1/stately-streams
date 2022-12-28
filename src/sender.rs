use std::pin::Pin;

use frunk::{hlist::HList, HCons, HNil};
use futures::{
    future::{join, ready, Join, Ready},
    pin_mut, Future,
};
use tokio::sync::mpsc::{error::SendError, Sender};

use crate::async_hlist::JoinFuture;

pub trait SenderHList<Item: Clone + 'static>
where
    Self: 'static,
{
    type SendResult;
    type Joined<'a>: JoinFuture;
    fn join<'a>(&'a self, item: Item) -> Self::Joined<'a>;
    fn send<'a>(&'a self, item: Item) -> <Self::Joined<'a> as JoinFuture>::JoinFuture {
        self.join(item).join_fut()
    }
}

impl<Item: Clone + 'static> SenderHList<Item> for HNil {
    type SendResult = HNil;

    type Joined<'a> = HNil;

    fn join<'a>(&'a self, _item: Item) -> Self::Joined<'a> {
        HNil
    }
}

trait NotNil {}

impl<H> NotNil for HCons<H, HNil> {}
impl<H, T: NotNil + HList> NotNil for HCons<H, T> {}

impl<Item: Clone + 'static, TailSender> SenderHList<Item> for HCons<Sender<Item>, TailSender>
where
    TailSender: HList + SenderHList<Item>,
{
    type SendResult = HCons<Result<(), SendError<Item>>, TailSender::SendResult>;
    type Joined<'a> = HCons<
        Pin<Box<dyn Future<Output = Result<(), SendError<Item>>> + 'a>>,
        TailSender::Joined<'a>,
    >;

    fn join<'a>(&'a self, item: Item) -> Self::Joined<'a> {
        let HCons { head, tail } = self;
        let head = Box::pin(head.send(item.clone()));

        HCons {
            head,
            tail: tail.join(item),
        }
    }
}

#[cfg(test)]
mod test {
    use frunk::hlist;
    use futures::{FutureExt, StreamExt};
    use tokio::sync::mpsc::channel;
    use tokio_stream::wrappers::ReceiverStream;

    use crate::async_hlist::JoinFuture;

    use super::SenderHList;
    #[tokio::test]
    async fn test_send() {
        let (t1, mut r1) = channel(2);
        let (t2, mut r2) = channel(2);
        let s = hlist![t1, t2];

        hlist![s.send(1i32), r1.recv(), r2.recv()].join_fut().await;
    }
}
