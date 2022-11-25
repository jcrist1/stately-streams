use frunk::{hlist::HList, HCons, HNil};
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;

use crate::async_coprod::SelectSubscribable;
use crate::async_hlist::JoinSubscribable;

pub trait SelectReceiverHList: Sized + ReceiverHList
where
    <Self as ReceiverHList>::Subscribable: SelectSubscribable,
{
    type SelectItems;
    fn select_stream(self) -> <Self::Subscribable as SelectSubscribable>::SubscriptionStream {
        self.subscribe().select_subscribe()
    }
}

pub trait JoinReceiverHList: Sized + ReceiverHList
where
    <Self as ReceiverHList>::Subscribable: JoinSubscribable,
{
    type JoinItems;
    fn join_stream(self) -> <Self::Subscribable as JoinSubscribable>::SubscriptionStream {
        self.subscribe().join_subscribe()
    }
}

impl<Receivers, JoinReceivers> JoinReceiverHList for Receivers
where
    Receivers: ReceiverHList<Subscribable = JoinReceivers>,
    JoinReceivers: JoinSubscribable,
{
    type JoinItems = JoinReceivers::SubscribedItem;
}

impl<Receivers, SelectReceivers> SelectReceiverHList for Receivers
where
    Receivers: ReceiverHList<Subscribable = SelectReceivers>,
    SelectReceivers: SelectSubscribable,
{
    type SelectItems = SelectReceivers::SubscribedItems;
}

pub trait ReceiverHList: Sized {
    type Subscribable: HList;
    fn subscribe(self) -> Self::Subscribable;
}

impl ReceiverHList for HNil {
    type Subscribable = HNil;
    fn subscribe(self) -> Self {
        self
    }
}

impl<HeadItem, TailReceivers> ReceiverHList for HCons<Receiver<HeadItem>, TailReceivers>
where
    HeadItem: Clone + 'static,
    TailReceivers: ReceiverHList + HList,
{
    type Subscribable = HCons<ReceiverStream<HeadItem>, TailReceivers::Subscribable>;

    fn subscribe(self) -> Self::Subscribable {
        let HCons { head, tail } = self;
        HCons {
            head: ReceiverStream::new(head),
            tail: tail.subscribe(),
        }
    }
}
