use std::{marker::PhantomData, ops::DerefMut, sync::MutexGuard};

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

pub trait Lock<FilterType: Filter>: 'static + Clone {
    type InnerType: MutRefHList + 'static;
    type LockType<'a>: AsMutHList<'a, Self::InnerType>
    where
        Self: 'a;
    fn lock<'a>(&'a self) -> Self::LockType<'a>;
}

impl Lock<HNil> for HNil {
    type InnerType = HNil;
    type LockType<'a> = HNil where HNil: 'a;
    fn lock<'a>(&'a self) -> HNil {
        HNil
    }
}

impl<S: SafeType + 'static, TailFilter: Filter, TailState: Lock<TailFilter>>
    Lock<HCons<True, TailFilter>> for HCons<SharedMutex<S>, TailState>
{
    type LockType<'a> = HCons<AsMutContainer<MutexGuard<'a, S>, S>, TailState::LockType<'a>>;

    type InnerType = HCons<S, TailState::InnerType>;

    fn lock<'a>(&'a self) -> Self::LockType<'a> {
        let HCons { head, tail } = self;
        let head = head.lock().unwrap();
        let tail = tail.lock();
        HCons {
            head: AsMutContainer::new(head),
            tail,
        }
    }
}

impl<S: SafeType + 'static, TailFilter: Filter, TailState: Lock<TailFilter>>
    Lock<HCons<False, TailFilter>> for HCons<SharedMutex<S>, TailState>
{
    type LockType<'a> = TailState::LockType<'a>;

    type InnerType = TailState::InnerType;

    fn lock<'a>(&'a self) -> Self::LockType<'a> {
        let HCons { tail, .. } = self;
        tail.lock()
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
pub struct AsMutContainer<RefType, Type> {
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
    #[allow(dead_code)]
    fn new(t: RefType) -> AsRefContainer<RefType, Type> {
        AsRefContainer {
            item: t,
            _type: PhantomData,
        }
    }
}

pub trait MutRefHList {
    type MutRefHList<'a>: 'a;
}

impl MutRefHList for HNil {
    type MutRefHList<'a> = HNil;
}

impl<Head: 'static, Tail: MutRefHList + 'static> MutRefHList for HCons<Head, Tail> {
    type MutRefHList<'a> = HCons<&'a mut Head, Tail::MutRefHList<'a>>;
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

pub trait AsMutHList<'a, MutRefHListType: MutRefHList>: 'a + Sized {
    fn mut_ref<'b>(&'b mut self) -> MutRefHListType::MutRefHList<'b>
    where
        'a: 'b;
    fn apply_fn<
        'b,
        InputType: 'static,
        OutputType: 'static,
        F: FnMut(MutRefHListType::MutRefHList<'b>, InputType) -> OutputType,
    >(
        &'b mut self,
        input: InputType,
        f: &'b mut F,
    ) -> OutputType
    where
        'a: 'b,
    {
        let mut_ref = self.mut_ref();
        let o = f(mut_ref, input);
        o
    }
}

impl<'a> AsMutHList<'a, HNil> for HNil {
    fn mut_ref<'b>(&'b mut self) -> HNil
    where
        'a: 'b,
    {
        HNil
    }
}

impl<'a, Head: 'static, Tail: 'static + MutRefHList, TailGuard: AsMutHList<'a, Tail>>
    AsMutHList<'a, HCons<Head, Tail>>
    for HCons<AsMutContainer<std::sync::MutexGuard<'a, Head>, Head>, TailGuard>
{
    fn mut_ref<'b>(&'b mut self) -> HCons<&'b mut Head, Tail::MutRefHList<'b>>
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

    #[test]
    fn test_apply_as_from_locked() {
        let state_1 = new_shared(1u32);
        let state_2 = new_shared(String::from("boop"));
        let state_3: SharedMutex<Vec<u32>> = new_shared(vec![]);
        let state = hlist!(state_1, state_2, state_3);
        let input = 4;
        let mut fun = |hlist_pat![u32_mut, string_mut, vec_mut]: HList!(
            &mut u32,
            &mut String,
            &mut Vec<u32>
        ),
                       input| {
            *u32_mut += 1u32;
            string_mut.push_str(&format!("{input}"));
            vec_mut.push(input)
        };
        let mut clos = |input| {
            let mut locked = Lock::<HList!(True, True, True)>::lock(&state);
            locked.apply_fn(input, &mut fun)
        };
        clos(input);
        clos(input);
    }

    #[test]
    fn test_build() {
        let state_1 = new_shared(1u32);
        let state_2 = new_shared(Some(String::from("boop")));
        let state_3 = new_shared(0.32);
        let state = hlist!(state_1, state_2, state_3);
        {
            println!("Starting first lock");
            let hlist_pat![_guard_1, _guard_2, _guard_3] =
                Lock::<HList![True, True, True]>::lock(&state);
        }
        {
            println!("Starting second lock");
            let hlist_pat![_guard_1, _guard_2] = Lock::<HList![True, True, False]>::lock(&state);
        }
        {
            println!("Starting third lock");
            let hlist_pat![_guard_1, _guard_2, _guard_3] = Lock::<
                <HList![True, True, False] as BoolAlg<HList![False, True, True]>>::Or,
            >::lock(&state);
        }
        {
            // this doesn't compile
            // let hlist_pat![guard_1, guard_2] = Lock::<HList![True, False, False]>::lock(&state);
        }
    }
}
