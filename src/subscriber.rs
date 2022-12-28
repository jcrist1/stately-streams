use frunk::{hlist, HCons, HList, HNil};
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct SubscriptionOutput<NewSenders, Item> {
    pub(crate) new_senders: NewSenders,
    pub(crate) new_subscription: Receiver<Item>,
}
pub trait Subscribable<Item> {
    type NewSenders;
    fn subscribe(self) -> SubscriptionOutput<Self::NewSenders, Item>;
}

impl<Item> Subscribable<Item> for HNil {
    type NewSenders = HList!(Sender<Item>);
    fn subscribe(self) -> SubscriptionOutput<HList!(Sender<Item>), Item> {
        let (tx, rx) = channel(2);
        SubscriptionOutput {
            new_senders: hlist![tx],
            new_subscription: rx,
        }
    }
}

impl<Item, TailSenders> Subscribable<Item> for HCons<Sender<Item>, TailSenders> {
    type NewSenders = HCons<Sender<Item>, Self>;

    fn subscribe(self) -> SubscriptionOutput<Self::NewSenders, Item> {
        let (tx, rx) = channel(2);
        SubscriptionOutput {
            new_senders: HCons {
                head: tx,
                tail: self,
            },
            new_subscription: rx,
        }
    }
}

#[cfg(test)]
mod test {}
