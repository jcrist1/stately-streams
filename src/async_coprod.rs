use futures::future::{
    pending, select as fu_select, Either, Map as FuMap, Pending, Select as FuSelect,
};

use frunk::coproduct::CNil;
use frunk::{coproduct::Coproduct, HCons, HNil};

use futures::stream::{empty, select as s_select, Empty};
use futures::stream::{Map as SMap, Select as SSelect};
use futures::{Future, Stream};
use futures::{FutureExt, StreamExt};

pub trait SelectSubscribable {
    type SubscribedItems;
    type SubscriptionStream: Stream<Item = Self::SubscribedItems>;
    fn select_subscribe(self) -> Self::SubscriptionStream;
}

impl SelectSubscribable for HNil {
    type SubscribedItems = CNil;

    type SubscriptionStream = Empty<CNil>;

    fn select_subscribe(self) -> Self::SubscriptionStream {
        empty()
    }
}

fn left<Head, Tail>(head: Head) -> Coproduct<Head, Tail> {
    Coproduct::Inl(head)
}
fn right<Head, Tail>(tail: Tail) -> Coproduct<Head, Tail> {
    Coproduct::Inr(tail)
}

impl<HeadStream, TailSubscribable, HeadItem, TailItems> SelectSubscribable
    for HCons<HeadStream, TailSubscribable>
where
    HeadStream: Stream<Item = HeadItem>,
    TailSubscribable: SelectSubscribable<SubscribedItems = TailItems>,
    HeadItem: 'static + Clone,
    TailItems: 'static + Clone,
{
    type SubscribedItems = Coproduct<HeadItem, TailItems>;
    type SubscriptionStream = SSelect<
        SMap<HeadStream, fn(HeadItem) -> Coproduct<HeadItem, TailItems>>,
        SMap<TailSubscribable::SubscriptionStream, fn(TailItems) -> Coproduct<HeadItem, TailItems>>,
    >;

    fn select_subscribe(self) -> Self::SubscriptionStream {
        let HCons { head, tail } = self;
        s_select(head.map(left), tail.select_subscribe().map(right))
    }
}

pub trait SelectFuture {
    type SelectOutput;
    type SelectFuture: Future<Output = Self::SelectOutput>;

    fn select_fut(self) -> Self::SelectFuture;
}
//
//impl SelectFuture for HNil {
//    type SelectOutput = CNil;
//    type SelectFuture = Pending<CNil>;
//    fn select_fut(self) -> Pending<CNil> {
//        pending()
//    }
//}
//
//fn either_to_coprod<L, R>(either: Either<L, R>) -> Coproduct<L, R> {
//    match either {
//        Either::Left(l) => left(l),
//        Either::Right(r) => right(r),
//    }
//}
//
//impl<HeadFuture, TailSelect, TailSelectFuture, HeadOutput, TailOutput> SelectFuture
//    for HCons<HeadFuture, TailSelect>
//where
//    HeadFuture: Future<Output = HeadOutput> + Unpin,
//    TailSelect: SelectFuture<SelectOutput = TailOutput, SelectFuture = TailSelectFuture>,
//    TailSelectFuture: Future<Output = TailOutput> + Unpin,
//{
//    type SelectOutput = Coproduct<HeadOutput, TailOutput>;
//    type SelectFuture = FuMap<
//        FuSelect<HeadFuture, TailSelect::SelectFuture>,
//        fn(Either<HeadOutput, TailOutput>) -> Coproduct<HeadOutput, TailOutput>,
//    >;
//
//    fn select_fut(self) -> Self::SelectFuture {
//        let HCons { mut head, tail } = self;
//
//        let tail = tail.select_fut();
//        FutureExt::map(fu_select(head, tail), either_to_coprod)
//    }
//}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use super::SelectSubscribable;
    use frunk::hlist;
    use frunk::Coprod;
    use futures::stream;
    use futures::StreamExt;
    use futures_timer::Delay;

    #[tokio::test]
    async fn test_anything() {
        Delay::new(Duration::from_millis(1500)).await;
        println!("Help");
        panic!("Help2")
    }
    #[tokio::test]
    async fn test_select() {
        type I64StringI8 = Coprod![i64, String, i8];
        println!("starting");

        let stream_1 = stream::once(async {
            println!("1");
            Delay::new(Duration::from_millis(30)).await;
            println!("1_2");
            1i64
        });
        let stream_2 = stream::once(async {
            println!("2");
            Delay::new(Duration::from_millis(20)).await;
            println!("2_2");
            String::from("Hello")
        });
        let stream_3 = stream::once(async {
            println!("3");
            Delay::new(Duration::from_millis(10)).await;
            println!("3_2");
            10i8
        });
        println!("starting");
        let run = hlist![stream_1, stream_2, stream_3]
            .select_subscribe()
            .collect::<Vec<_>>()
            .await;
        println!("Done");

        assert_eq!(
            run,
            vec![
                I64StringI8::inject(10i8),
                I64StringI8::inject(String::from("Hello")),
                I64StringI8::inject(1i64)
            ]
        );
    }
}
