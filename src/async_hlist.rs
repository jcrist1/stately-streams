use frunk::{hlist::HList, HCons, HNil};
use futures::future::{
    join as fu_join, ready as fu_ready, Join as FuJoin, Map as FuMap, Ready as FuReady,
};
use futures::{
    stream::{self, Map, Repeat, Zip},
    Stream,
};
use futures::{Future, FutureExt, StreamExt};

pub trait JoinSubscribable {
    type SubscribedItem: HList;
    type SubscriptionStream: Stream<Item = Self::SubscribedItem>;
    fn join_subscribe(self) -> Self::SubscriptionStream;
}

impl<'a> JoinSubscribable for HNil {
    type SubscribedItem = HNil;
    type SubscriptionStream = Repeat<HNil>;

    fn join_subscribe(self) -> Repeat<HNil> {
        stream::repeat(HNil)
    }
}

fn tuple_to_hlist<Head, Tail>((head, tail): (Head, Tail)) -> HCons<Head, Tail> {
    HCons { head, tail }
}

impl<'a, HeadStream, HeadItem, TailSubscribable, TailItems> JoinSubscribable
    for HCons<HeadStream, TailSubscribable>
where
    HeadStream: Stream<Item = HeadItem>,
    TailSubscribable: JoinSubscribable<SubscribedItem = TailItems>,
    HeadItem: 'static,
    TailItems: HList + 'static,
{
    type SubscribedItem = HCons<HeadItem, TailItems>;

    type SubscriptionStream = Map<
        Zip<HeadStream, TailSubscribable::SubscriptionStream>,
        fn((HeadItem, TailItems)) -> HCons<HeadItem, TailItems>,
    >;

    fn join_subscribe(self) -> Self::SubscriptionStream {
        let HCons { head, tail } = self;
        StreamExt::zip(head, tail.join_subscribe()).map(tuple_to_hlist)
    }
}

pub trait JoinFuture {
    type JoinOutput;
    type JoinFuture: Future<Output = Self::JoinOutput>;

    fn join_fut(self) -> Self::JoinFuture;
}

impl JoinFuture for HNil {
    type JoinOutput = HNil;
    type JoinFuture = FuReady<HNil>;

    fn join_fut(self) -> Self::JoinFuture {
        fu_ready(self)
    }
}

impl<HeadFuture, HeadOutput, TailJoinFuture, TailOutput> JoinFuture
    for HCons<HeadFuture, TailJoinFuture>
where
    HeadFuture: Future<Output = HeadOutput>,
    TailJoinFuture: JoinFuture<JoinOutput = TailOutput>,
{
    type JoinOutput = HCons<HeadOutput, TailOutput>;

    type JoinFuture = FuMap<
        FuJoin<HeadFuture, TailJoinFuture::JoinFuture>,
        fn((HeadOutput, TailOutput)) -> HCons<HeadOutput, TailOutput>,
    >;

    fn join_fut(self) -> Self::JoinFuture {
        fu_join(self.head, self.tail.join_fut()).map(tuple_to_hlist)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use frunk::hlist;
    use futures::{stream, StreamExt};
    use futures_timer::Delay;

    use super::JoinSubscribable;

    #[tokio::test]
    async fn test_join() {
        let stream_1 = stream::once(async {
            Delay::new(Duration::from_millis(30)).await;
            1i32
        });
        let stream_2 = stream::once(async {
            Delay::new(Duration::from_millis(20)).await;
            2i8
        });
        let stream_3 = stream::once(async {
            Delay::new(Duration::from_millis(10)).await;
            3i16
        });

        let run = hlist![stream_1, stream_2, stream_3]
            .join_subscribe()
            .collect::<Vec<_>>()
            .await;
        assert_eq!(vec![hlist!(1i32, 2i8, 3i16)], run)
    }
}
