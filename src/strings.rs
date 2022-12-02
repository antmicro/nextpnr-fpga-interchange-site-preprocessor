/* Copyright (C) 2022 Antmicro
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *     https://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::collections::HashMap;
use std::sync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::borrow::Borrow;

use lazy_static::__Deref;

lazy_static!{
    static ref GLOBAL_STRINGS: RwLock<Vec<String>> = RwLock::new(Vec::new());
    static ref GLOBAL_STRINGS_REVMAP: Mutex<HashMap<String, usize>> =
        Mutex::new(HashMap::new());
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GlobalStringId(usize);

pub struct GlobalStringsCtx();

/* We need some sort of an "object" to mark the scope in which we hold the reference
 * to a string. See the `'s` lifetime in `Self::get_global_string` */
impl GlobalStringsCtx {
    pub fn hold() -> Self {
        Self()
    }

    /// Get a global identifier for a provided string. Creates a new identifier if the
    /// string was not registered. Returns an existing identifier if the string has been
    /// already registered.
    pub fn create_global_string<S>(&mut self, s: S) -> GlobalStringId where
        S: ToString + Borrow<str>
    {
        /* Use &mut self reference to statically prevent deadlocking with
         * `Self::get_global_string` */

        /* We need to acquire exclusive lock on `GLOBAL_STRINGS_REVMAP` first to prevent
         * failing the ID presence check in one thread, then adding the same ID in another,
         * one and then adding a duplicate in this one */
        let mut revmap = GLOBAL_STRINGS_REVMAP.lock().unwrap();
    
        if let Some(id) = revmap.get(s.borrow()) {
            return GlobalStringId(*id);
        }
    
        let mut strings = GLOBAL_STRINGS.write().unwrap();
        
        let id = strings.len();

        let s = s.to_string();
        revmap.insert(s.clone(), id);
        strings.push(s);
    
        GlobalStringId(id)
    }

    pub fn get_global_string<'s>(&'s self, id: GlobalStringId) -> GlobalStringRef<'s> {
        GlobalStringRef {
            guard: GLOBAL_STRINGS.read().unwrap(),
            idx: id.0
        }
    }
}

pub struct GlobalStringRef<'l> {
    guard: RwLockReadGuard<'l, Vec<String>>,
    idx: usize,
}

impl<'l> std::ops::Deref for GlobalStringRef<'l> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.guard[self.idx]
    }
}

impl<'l> Borrow<str> for GlobalStringRef<'l> {
    fn borrow(&self) -> &str {
        &self.guard[self.idx]
    }
}

impl<'l> std::fmt::Debug for GlobalStringRef<'l> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GlobalStringRef({})", self.guard[self.idx])
    }
}

impl<'l> std::fmt::Display for GlobalStringRef<'l> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.guard[self.idx].fmt(f)
    }
}

impl<'l> std::hash::Hash for GlobalStringRef<'l> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.idx.hash(state)
    }
}

impl<'l> std::cmp::PartialEq for GlobalStringRef<'l> {
    fn eq(&self, other: &Self) -> bool {
        self.idx.eq(&other.idx)
    }
}

impl<'l> std::cmp::Eq for GlobalStringRef<'l> {}

pub struct GlobalStringRefMut<'l> {
    guard: RwLockWriteGuard<'l, Vec<String>>,
    idx: usize,
}

impl<'l> std::ops::Deref for GlobalStringRefMut<'l> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.guard[self.idx]
    }
}

impl<'l> std::ops::DerefMut for GlobalStringRefMut<'l> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard[self.idx]
    }
}

impl<'l> std::fmt::Debug for GlobalStringRefMut<'l> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GlobalStringRef({})", self.guard[self.idx].deref())
    }
}

impl<'l> std::fmt::Display for GlobalStringRefMut<'l> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.guard[self.idx].fmt(f)
    }
}

impl<'l> std::hash::Hash for GlobalStringRefMut<'l> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.idx.hash(state)
    }
}

impl<'l> std::cmp::PartialEq for GlobalStringRefMut<'l> {
    fn eq(&self, other: &Self) -> bool {
        self.idx.eq(&other.idx)
    }
}

impl<'l> std::cmp::Eq for GlobalStringRefMut<'l> {}
