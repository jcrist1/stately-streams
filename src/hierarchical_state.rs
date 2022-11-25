use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::MutexGuard,
};

use frunk::{hlist::HList, HCons, HNil};

use crate::util::{SafeType, SharedMutex};
trait HierarchicalState {}

pub struct True;
pub struct False;
pub trait TypeBool {
    type Or<T: TypeBool>: TypeBool;
    type And<T: TypeBool>: TypeBool;
    fn bool() -> bool;
}
impl TypeBool for True {
    type Or<T: TypeBool> = True;
    type And<T: TypeBool> = T;
    fn bool() -> bool {
        true
    }
}
impl TypeBool for False {
    type Or<T: TypeBool> = T;
    type And<T: TypeBool> = False;
    fn bool() -> bool {
        false
    }
}

fn or<T: TypeBool>(_t: T) -> True {
    True
}

trait InductiveStateSubset {}

pub trait Counter: HList {}

impl Counter for HNil {}
impl<Tail: Counter> Counter for HCons<(), Tail> {}

pub trait Countable: HList {
    type Count: Counter;
}

impl Countable for HNil {
    type Count = HNil;
}

impl<Head, Tail: Countable> Countable for HCons<Head, Tail> {
    type Count = HCons<(), <Tail as Countable>::Count>;
}

pub trait SharedState: Countable {
    fn clone(&self) -> Self;
}

pub trait Filter: Countable {}

impl Filter for HNil {}

impl<H: TypeBool, T: Filter> Filter for HCons<H, T> {}

trait BoolAlg<Other: Filter>: Filter {
    type Or: Filter;
    type And: Filter;
}

impl BoolAlg<HNil> for HNil {
    type Or = HNil;
    type And = HNil;
}

impl<H1: TypeBool, T1: Filter + BoolAlg<T2>, H2: TypeBool, T2: Filter> BoolAlg<HCons<H2, T2>>
    for HCons<H1, T1>
{
    type Or = HCons<H1::Or<H2>, T1::Or>;
    type And = HCons<H1::And<H2>, T1::And>;
}

trait POSet<Other> {
    type IsSubset: TypeBool;

    fn is_subset(&self) -> bool {
        Self::IsSubset::bool()
    }
}

impl POSet<HNil> for HNil {
    type IsSubset = True;
}

impl<H1: TypeBool, T1: Filter + BoolAlg<T2> + POSet<T2>, H2: TypeBool, T2: Filter>
    POSet<HCons<H2, T2>> for HCons<H1, T1>
{
    type IsSubset =
        <<H2 as TypeBool>::And<<H1 as TypeBool>::Or<H2>> as TypeBool>::And<T1::IsSubset>;
}

pub trait Lock<FilterType: Filter>: 'static {
    type InnerType;
    type LockType<'a>: AsMutHList<'a>
    where
        Self: 'a;
    type RefType<'a>: 'a;
    fn lock<'a>(&'a self) -> Self::LockType<'a>;

    fn apply_fn<
        'a,
        InputType: 'static,
        OutputType: 'static,
        F: for<'d> Fn(Self::RefType<'d>, InputType) -> OutputType,
    >(
        &'a self,
        input: InputType,
        f: F,
    ) -> OutputType;
}

impl Lock<HNil> for HNil {
    type InnerType = HNil;
    type LockType<'a> = HNil where HNil: 'a;
    fn lock<'a>(&'a self) -> HNil {
        HNil
    }

    type RefType<'a> = <Self::LockType<'a> as AsMutHList<'a>>::AsMutType<'a>;
    fn apply_fn<
        'a,
        InputType: 'static,
        OutputType: 'static,
        F: for<'d> Fn(Self::RefType<'d>, InputType) -> OutputType,
    >(
        &'a self,
        input: InputType,
        f: F,
    ) -> OutputType {
        let mut guard = self.lock();
        guard.apply_fn(input, f)
    }
}

impl<S: SafeType + 'static, TailFilter: Filter, TailState: Lock<TailFilter>>
    Lock<HCons<True, TailFilter>> for HCons<SharedMutex<S>, TailState>
{
    type LockType<'a> = HCons<AsMutContainer<MutexGuard<'a, S>, S>, TailState::LockType<'a>>;

    type InnerType = HCons<S, TailState::InnerType>;
    type RefType<'a> = <Self::LockType<'a> as AsMutHList<'a>>::AsMutType<'a>;

    fn lock<'a>(&'a self) -> Self::LockType<'a> {
        let HCons { head, tail } = self;
        let head = head.lock().unwrap();
        let tail = tail.lock();
        HCons {
            head: AsMutContainer::new(head),
            tail,
        }
    }
    fn apply_fn<
        'a,
        InputType: 'static,
        OutputType: 'static,
        F: for<'d> Fn(Self::RefType<'d>, InputType) -> OutputType,
    >(
        &'a self,
        input: InputType,
        f: F,
    ) -> OutputType {
        let mut guard = <Self as Lock<HCons<True, TailFilter>>>::lock(self);
        guard.apply_fn(input, f)
    }
}

impl<S: SafeType + 'static, TailFilter: Filter, TailState: Lock<TailFilter>>
    Lock<HCons<False, TailFilter>> for HCons<SharedMutex<S>, TailState>
{
    type LockType<'a> = TailState::LockType<'a>;

    type InnerType = TailState::InnerType;
    type RefType<'a> = <Self::LockType<'a> as AsMutHList<'a>>::AsMutType<'a>;

    fn lock<'a>(&'a self) -> Self::LockType<'a> {
        let HCons { tail, .. } = self;
        tail.lock()
    }
    fn apply_fn<
        'a,
        InputType: 'static,
        OutputType: 'static,
        F: for<'d> Fn(Self::RefType<'d>, InputType) -> OutputType,
    >(
        &'a self,
        input: InputType,
        f: F,
    ) -> OutputType {
        let mut guard = <Self as Lock<HCons<False, TailFilter>>>::lock(self);
        guard.apply_fn(input, f)
    }
}

trait AsRefHList<'b>: 'b {
    type AsRefType<'a>: 'a
    where
        'b: 'a;
    fn as_ref<'a>(&'a self) -> Self::AsRefType<'a>
    where
        'b: 'a;
}

impl<'b> AsRefHList<'b> for HNil {
    type AsRefType<'a> = HNil where 'b: 'a;
    fn as_ref<'a>(&'a self) -> Self::AsRefType<'a>
    where
        'b: 'a,
    {
        HNil
    }
}
struct AsMutContainer<RefType, Type> {
    item: RefType,
    _type: PhantomData<Type>,
}

impl<Type, RefType: DerefMut<Target = Type>> AsMutContainer<RefType, Type> {
    fn new(t: RefType) -> AsMutContainer<RefType, Type> {
        AsMutContainer {
            item: t,
            _type: PhantomData,
        }
    }
}

struct AsRefContainer<RefType, Type> {
    item: RefType,
    _type: PhantomData<Type>,
}

impl<Type, RefType: AsRef<Type>> AsRefContainer<RefType, Type> {
    fn new(t: RefType) -> AsRefContainer<RefType, Type> {
        AsRefContainer {
            item: t,
            _type: PhantomData,
        }
    }
}

impl<'b, Head: 'static, HeadRef: 'b + AsRef<Head>, Tail: AsRefHList<'b>> AsRefHList<'b>
    for HCons<AsRefContainer<HeadRef, Head>, Tail>
{
    type AsRefType<'a> = HCons<&'a Head, Tail::AsRefType<'a>> where 'b: 'a;

    fn as_ref<'a>(&'a self) -> Self::AsRefType<'a>
    where
        'b: 'a,
    {
        let HCons { head, tail } = self;
        HCons {
            head: head.item.as_ref(),
            tail: tail.as_ref(),
        }
    }
}

pub trait AsMutHList<'a>: 'a {
    type AsMutType<'b>: 'b
    where
        'a: 'b;
    fn mut_ref<'b>(&'b mut self) -> Self::AsMutType<'b>
    where
        'a: 'b;

    fn apply_fn<
        'b,
        InputType: 'static,
        OutputType: 'static,
        F: for<'d> Fn(Self::AsMutType<'d>, InputType) -> OutputType,
    >(
        &'b mut self,
        input: InputType,
        f: F,
    ) -> OutputType {
        let mut_ref = self.mut_ref();
        f(mut_ref, input)
    }
}

impl<'a> AsMutHList<'a> for HNil {
    type AsMutType<'b> = HNil where 'a: 'b;
    fn mut_ref<'b>(&'b mut self) -> Self::AsMutType<'b>
    where
        'a: 'b,
    {
        HNil
    }
}

impl<'a, Head: 'static, HeadMutRef: DerefMut<Target = Head> + 'a, Tail: AsMutHList<'a>>
    AsMutHList<'a> for HCons<AsMutContainer<HeadMutRef, Head>, Tail>
{
    type AsMutType<'b> = HCons<&'b mut Head, Tail::AsMutType<'b>> where 'a: 'b;

    fn mut_ref<'b>(&'b mut self) -> Self::AsMutType<'b>
    where
        'a: 'b,
    {
        let HCons {
            head: AsMutContainer { item, .. },
            tail,
        } = self;
        HCons {
            head: item.deref_mut(),
            tail: tail.mut_ref(),
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::Mutex;

    use frunk::{hlist, hlist_pat};

    use super::AsMutContainer;
    use super::AsMutHList;
    use super::AsRefContainer;
    use super::AsRefHList;
    use frunk::HList;

    use super::*;
    use crate::util::new_shared;

    #[test]
    fn test_as_mut() {
        let s1 = Mutex::new(String::from("Hello"));
        let vec2 = Mutex::new(vec![0, 1, 2]);
        let int = Mutex::new(3i64);
        let mut h_list = hlist![
            AsMutContainer::new(s1.lock().unwrap()),
            AsMutContainer::new(vec2.lock().unwrap()),
            AsMutContainer::new(int.lock().unwrap())
        ];
        let hlist_pat!(_, _, _) = h_list.mut_ref();
    }
    #[test]
    fn test_as_ref() {
        let s1 = Arc::new(String::from("Hello"));
        let vec2 = Arc::new(vec![0, 1, 2]);
        let int = Arc::new(3i64);
        let h_list = hlist![
            AsRefContainer::new(s1),
            AsRefContainer::new(vec2),
            AsRefContainer::new(int)
        ];
        {
            let hlist_pat!(_, _, _) = h_list.as_ref();
        }
    }

    // need to uncomment #[test]
    fn test_build() {
        let state_1 = new_shared(1u32);
        let state_2 = new_shared(Some(String::from("boop")));
        let state_3 = new_shared(0.32);
        let state = hlist!(state_1, state_2, state_3);
        println!("Starting first lock");
        let hlist_pat![_guard_1, _guard_2, _guard_3] =
            Lock::<HList![True, True, True]>::lock(&state);
        println!("Starting second lock");
        let hlist_pat![_guard_1, _guard_2] = Lock::<HList![True, True, False]>::lock(&state);
        let hlist_pat![_guard_1, _guard_2, _guard_3] = Lock::<
            <HList![True, True, False] as BoolAlg<HList![False, True, True]>>::Or,
        >::lock(&state);
        // this doesn't compile
        // let hlist_pat![guard_1, guard_2] = Lock::<HList![True, False, False]>::lock(&state);
    }
}
