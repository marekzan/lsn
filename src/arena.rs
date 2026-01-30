pub struct Arena<T> {
    data: Vec<Slot<T>>,
    free_slot: Option<usize>,
    count: u64,
}

/// we return this to the caller.
/// this can be used to get data back from the arena
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    pub index: usize,
    pub generation: u64,
}

/// a slot represents an already used (Free) space in the data vec or a currently occupied space
///
/// if it is occupied, the slot holds the actual data which the user can get back using the handle
/// and the current generation of this slot.
///
/// if the slot is free then it points to the next free index (which might
/// be another free slot or a true empty index in the vec) with their current
/// generation
enum Slot<T> {
    Occupied {
        value: T,
        generation: u64,
    },
    Free {
        next_free: Option<usize>,
        generation: u64,
    },
}

impl<T> Arena<T> {
    // when creating a new arena, we initialize the vec and since there are not free slots,
    // we set it to None and the count to 0.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            free_slot: None,
            count: 0,
        }
    }

    pub fn insert(&mut self, value: T) -> Handle {
        let (index, generation) = if let Some(idx) = self.free_slot {
            // get current generation and the next free index from the current free slot
            let (free_slot, generation) = match &self.data[idx] {
                Slot::Free {
                    next_free: free_index,
                    generation,
                } => (*free_index, *generation),
                Slot::Occupied { .. } => panic!("Corrupt free list"),
            };

            self.free_slot = free_slot;
            self.data[idx] = Slot::Occupied { value, generation };

            (idx, generation)

        // not free slot so we create a new occupied one
        } else {
            let idx = self.data.len();

            self.data.push(Slot::Occupied {
                value,
                generation: 0,
            });

            (idx, 0)
        };

        self.count += 1;
        Handle { index, generation }
    }

    pub fn remove(&mut self, handle: Handle) -> Option<T> {
        if self.get(&handle).is_none() {
            return None;
        }

        let new_free_slot = Slot::<T>::Free {
            next_free: self.free_slot,
            generation: handle.generation + 1,
        };

        let old_slot = std::mem::replace(&mut self.data[handle.index], new_free_slot);

        self.free_slot = Some(handle.index);
        self.count -= 1;

        match old_slot {
            Slot::Occupied { value, .. } => Some(value),
            Slot::Free { .. } => panic!("should not happen"),
        }
    }

    pub fn get(&self, handle: &Handle) -> Option<&T> {
        if handle.index >= self.data.len() {
            return None;
        }

        match &self.data[handle.index] {
            Slot::Occupied { generation, value } if *generation == handle.generation => {
                return Some(value);
            }
            _ => return None,
        }
    }

    pub fn get_mut(&mut self, handle: &Handle) -> Option<&mut T> {
        if handle.index >= self.data.len() {
            return None;
        }

        match &mut self.data[handle.index] {
            Slot::Occupied { generation, value } if *generation == handle.generation => {
                return Some(value);
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut arena = Arena::new();
        let h1 = arena.insert(10);
        let h2 = arena.insert(20);

        assert_eq!(arena.get(&h1), Some(&10));
        assert_eq!(arena.get(&h2), Some(&20));
    }

    #[test]
    fn test_remove() {
        let mut arena = Arena::new();
        let h1 = arena.insert(10);
        assert_eq!(arena.remove(h1), Some(10));
        assert_eq!(arena.get(&h1), None);
        // Double remove should fail
        assert_eq!(arena.remove(h1), None);
    }

    #[test]
    fn test_reuse_slot() {
        let mut arena = Arena::new();
        let h1 = arena.insert(10);
        let idx = h1.index;
        let old_gen = h1.generation;

        arena.remove(h1);

        let h2 = arena.insert(30);
        assert_eq!(h2.index, idx); // Should reuse index
        assert!(h2.generation > old_gen); // Generation should increase
        assert_eq!(arena.get(&h2), Some(&30));
    }

    #[test]
    fn test_stale_handle() {
        let mut arena = Arena::new();
        let h1 = arena.insert(10);
        arena.remove(h1);

        // Slot reused
        let h2 = arena.insert(20);

        // Old handle should not access new data
        assert_eq!(arena.get(&h1), None);
        // New handle should work
        assert_eq!(arena.get(&h2), Some(&20));
    }

    #[test]
    fn test_get_mut() {
        let mut arena = Arena::new();
        let h1 = arena.insert("hello".to_string());

        if let Some(val) = arena.get_mut(&h1) {
            val.push_str(" world");
        }

        assert_eq!(arena.get(&h1).map(|s| s.as_str()), Some("hello world"));
    }

    #[test]
    fn test_multiple_generations() {
        let mut arena = Arena::new();
        let h1 = arena.insert(1);

        arena.remove(h1);
        let h2 = arena.insert(2);

        arena.remove(h2);
        let h3 = arena.insert(3);

        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
        assert_ne!(h1, h3);

        assert_eq!(arena.get(&h1), None);
        assert_eq!(arena.get(&h2), None);
        assert_eq!(arena.get(&h3), Some(&3));
    }
}

#[cfg(test)]
mod proptest {

    use crate::{Arena, Handle};
    use proptest::prelude::*;
    use std::collections::HashMap;

    #[derive(Debug, Clone)]
    enum Action {
        Insert(u32),
        Remove(usize),
        Get(usize),
        GetMut(usize, u32),
        GetInvalid(usize, u64),
    }

    proptest! {
        #[test]
        fn test_arena_matches_hashmap(actions in prop::collection::vec(
            prop_oneof![
                any::<u32>().prop_map(Action::Insert),
                any::<usize>().prop_map(Action::Remove),
                any::<usize>().prop_map(Action::Get),
                (any::<usize>(), any::<u32>()).prop_map(|(idx, val)| Action::GetMut(idx, val)),
                (any::<usize>(), any::<u64>()).prop_map(|(idx, generation_id)| Action::GetInvalid(idx, generation_id)),
            ],
            0..400
        )) {
            let mut arena = Arena::new();
            let mut model = HashMap::new();
            let mut handles = Vec::new();

            for action in actions {
                match action {
                    Action::Insert(val) => {
                        let handle = arena.insert(val);
                        model.insert(handle, val);
                        handles.push(handle);
                    }
                    Action::Remove(idx) => {
                        if !handles.is_empty() {
                            let idx = idx % handles.len();
                            let handle = handles[idx];

                            let arena_res = arena.remove(handle);
                            let model_res = model.remove(&handle);

                            assert_eq!(arena_res, model_res, "Remove mismatch for handle {:?}", handle);
                        }
                    }
                    Action::Get(idx) => {
                        if !handles.is_empty() {
                            let idx = idx % handles.len();
                            let handle = handles[idx];

                            let arena_res = arena.get(&handle);
                            let model_res = model.get(&handle);

                            assert_eq!(arena_res, model_res, "Get mismatch for handle {:?}", handle);
                        }
                    }
                    Action::GetMut(idx, new_val) => {
                        if !handles.is_empty() {
                            let idx = idx % handles.len();
                            let handle = handles[idx];

                            if let Some(val) = model.get_mut(&handle) {
                                *val = new_val;
                            }

                            if let Some(val) = arena.get_mut(&handle) {
                                *val = new_val;
                            }

                            let arena_res = arena.get(&handle);
                            let model_res = model.get(&handle);
                            assert_eq!(arena_res, model_res, "GetMut (post-update) mismatch for handle {:?}", handle);
                        }
                    }
                    Action::GetInvalid(idx, generation_id) => {
                         // We construct a handle that is LIKELY invalid.
                         // However, by random chance, it *could* match an existing valid handle if we are unlucky.
                         // So we check our model to see if it SHOULD exist.
                         let forged_handle = Handle { index: idx, generation: generation_id };

                         let arena_res = arena.get(&forged_handle);
                         let model_res = model.get(&forged_handle);

                         assert_eq!(arena_res, model_res, "Forged handle {:?} mismatch", forged_handle);
                    }
                }
            }
        }
    }
}
