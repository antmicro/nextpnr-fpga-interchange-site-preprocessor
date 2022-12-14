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


/* The representations  might be suboptimal at the moment
 * What's important is the functionality. Optimisations can come later.
 */

use std::cmp::Ordering;

mod intersperse;
#[cfg(test)]
mod tests;

use self::intersperse::*;
#[allow(unused)]
use crate::log::*;

#[derive(Serialize, Deserialize)]
pub enum FormulaTerm<Id> where Id: Ord + Eq {
    Var(Id),
    NegVar(Id),
    True,
    False,
}

impl<Id> FormulaTerm<Id> where Id: Ord + Eq {
    /// Check if term is negation of the other term
    pub fn neg_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Var(a), Self::NegVar(b)) => a == b,
            (Self::NegVar(a), Self::Var(b)) => a == b,
            (Self::True, Self::False) | (Self::False, Self::True) => true,
            _ => false
        }
    }

    pub fn map<F, NId>(self, f: F) -> FormulaTerm<NId> where
        F: FnOnce(Id) -> NId,
        NId: Ord + Eq
    {
        match self {
            Self::Var(v) => FormulaTerm::Var(f(v)),
            Self::NegVar(v) => FormulaTerm::NegVar(f(v)),
            Self::True => FormulaTerm::True,
            Self::False => FormulaTerm::False,
        }
    }
}

impl<Id> std::fmt::Debug for FormulaTerm<Id> where Id: Ord + Eq + std::fmt::Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Var(a) => a.fmt(f),
            Self::NegVar(a) => write!(f, "¬{:?}", a),
            Self::True => write!(f, "⊤"),
            Self::False => write!(f, "⊥"),
        }
    }
}

impl<Id> PartialEq for FormulaTerm<Id> where Id: Ord + Eq {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Var(a), Self::Var(b)) => a == b,
            (Self::NegVar(a), Self::NegVar(b)) => a == b,
            (Self::False, Self::False) => true,
            (Self::True, Self::True) => true,
            _ => false
        }
    }
}

impl<Id> Eq for FormulaTerm<Id> where Id: Ord + Eq {}

/* The order goes like:
 *   ⊥ < ... x < y < ¬y < z < ... < ⊤
 * So False goes first (to allow quickly determining that the entire conjunction
 * group evaluates to false), then there are variables sorted according to their order
 * with negated variables sitting next to their non-negates counterparts (again, to
 * allow quickly determining that the group evaluates to false), than the last element
 * is True (because it's neutral).
 */
impl<Id> PartialOrd for FormulaTerm<Id> where Id: Ord + Eq {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let o = match (self, other) {
            (Self::False, Self::False) => Ordering::Equal,
            (Self::False, _) => Ordering::Less,
            (_, Self::False) => Ordering::Greater,
            (Self::True, Self::True) => Ordering::Equal,
            (Self::True, _) => Ordering::Equal,
            (_, Self::True) => Ordering::Less,
            (Self::Var(a) | Self::NegVar(a), Self::Var(b) | Self::NegVar(b)) => {
                match a.cmp(b) {
                    Ordering::Equal => {
                        match (self, other) {
                            (Self::Var(_), Self::NegVar(_)) => Ordering::Less,
                            (Self::NegVar(_), Self::Var(_)) => Ordering::Greater,
                            _ => Ordering::Equal
                        }
                    },
                    o @ _ => o,
                }
            }
        };

        Some(o)
    }
}

impl<Id> Ord for FormulaTerm<Id> where Id: Ord + Eq {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<Id> Clone for FormulaTerm<Id> where Id: Ord + Eq + Clone {
    fn clone(&self) -> Self {
        match self {
            Self::Var(v) => Self::Var(v.clone()),
            Self::NegVar(v) => Self::NegVar(v.clone()),
            Self::True => Self::True,
            Self::False => Self::False,
        }
    }
}

/// Represents a conjunction group (aka. "cube") in DNF boolean formula
#[derive(PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DNFCube<Id> where Id: Ord + Eq {
    pub terms: Vec<FormulaTerm<Id>>
}

impl<Id> DNFCube<Id> where Id: Ord + Eq {
    pub fn new() -> Self {
        Self { terms: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_true_const(&self) -> bool {
        self.terms.iter().find(|term| {
            if let FormulaTerm::True = term {
                false
            } else {
                true
            }
        }).is_none()
    }

    pub fn is_false_const(&self) -> bool {
        self.terms.contains(&FormulaTerm::False)
    }

    pub fn add_term(&mut self, term: FormulaTerm<Id>) {
        /* Could be done faster in terms of time complexity */
        let idx = {
            let mut my_term_idx = 0;
            loop {
                if my_term_idx == self.terms.len() { break my_term_idx; }
                let my_term = &self.terms[my_term_idx];
                if &term < my_term { break my_term_idx; }
                if &term == my_term { return; }
                /* p ∧ ¬p ∧ ... ≡ ⊥ */
                if term.neg_eq(my_term) {
                    self.terms.clear();
                    self.terms.push(FormulaTerm::False);
                    return;
                }
                my_term_idx += 1;
            }
        };
        self.terms.insert(idx, term);
    }

    pub fn map<F, NId>(self, mut f: F) -> DNFCube<NId> where
        F: FnMut(Id) -> NId,
        NId: Ord + Eq
    {
        DNFCube {
            terms: self.terms.into_iter().map(|t| {
                match t {
                    FormulaTerm::Var(v) => FormulaTerm::Var(f(v)),
                    FormulaTerm::NegVar(v) => FormulaTerm::NegVar(f(v)),
                    FormulaTerm::True => FormulaTerm::True,
                    FormulaTerm::False => FormulaTerm::False,
                }
            }).collect()
        }
    }

    /// Returns `true` if every interpretation that satisfies this cube
    /// also satisfies the other cube. Otherwise, returns `false`.
    pub fn is_subcube(&self, other: &Self) -> bool {
        if other.terms.len() > self.terms.len() { return false; }
        self.terms.iter().zip(other.terms.iter())
            .find(|(my_term, other_term)| other_term < my_term)
            .is_none()
    }
}

pub trait ReductibleDNFCube<Id> where
    Self: Sized + std::fmt::Debug,
{
    /// Attempts to reduce disjunction of two cubes into a single cube
    fn try_to_reduce_disjunction(&self, other: &Self) -> Option<Self>;
}

enum TermReductionAction {
    Move,   /* Moves the term to the reduced form */
    Skip,   /* Skips the term */
    Ignore, /* Ignores the term in this iteration. To be checked in the next one */
}

impl<Id> ReductibleDNFCube<Id> for DNFCube<Id> where
    Self: std::fmt::Debug,
    FormulaTerm<Id>: std::fmt::Debug,
    Id: Ord + Eq + Clone,
{
    /* This is ridiculously complex but the goal is to do it in linear time. */
    fn try_to_reduce_disjunction(&self, other: &Self) -> Option<Self> {
        dbg_log!(DBG_EXTRA2, "Reducing disjunction between {:?} and {:?}", self, other);
        /* Reduced cube in construction */
        let mut reduced = DNFCube::new();

        /* Becomes true, when the reduced cube prefix turns out to be less
         * strict than any of the input cubes prefixes */
        let mut less_strict_than_me = false;
        let mut less_strict_than_other = false;
        
        /* Note: terms in cubes must be sorted! */
        let mut my_it = self.terms.iter().peekable();
        let mut others_it = other.terms.iter().peekable();
        
        /* Traverse sorted cubes to check for same variables, their negations
         * and constants */
        let mut my_yield = my_it.next();
        let mut others_yield = others_it.next();
        let reductible: bool = 'fsm: loop { 
            #[allow(unused_assignments)]
            let mut take_me = TermReductionAction::Move;
            #[allow(unused_assignments)]
            let mut take_other = TermReductionAction::Move;

            dbg_log!(DBG_EXTRA2, "Yielded {:?}, {:?}", my_yield, others_yield);

            /* Perform reductions */
            match (my_yield, others_yield) {
                /* (⊥ ∧ ∧{x}) ∨ ∧{y} ≡ ∧{y} */
                (Some(FormulaTerm::False), _) => {
                    dbg_log!(DBG_EXTRA2, "(⊥ ∧ ∧{{x}}) ∨ ∧{{y}} ≡ ∧{{y}}");
                    return Some(other.clone());
                },
                (_, Some(FormulaTerm::False)) => {
                    dbg_log!(DBG_EXTRA2, "∧{{y}} ∨ (⊥ ∧ ∧{{x}}) ≡ ∧{{y}}");
                    return Some(self.clone());
                }
                /* ⊤ ∨ ⊤ ≡ ⊤ */
                (Some(FormulaTerm::True), Some(FormulaTerm::True)) => {
                    dbg_log!(DBG_EXTRA2, "⊤ ∨ ⊤ ≡ ⊤");
                    take_other = TermReductionAction::Move;
                    take_me = TermReductionAction::Skip;
                },
                (Some(FormulaTerm::Var(x1)), Some(FormulaTerm::Var(x2)))
                | (Some(FormulaTerm::NegVar(x1)), Some(FormulaTerm::NegVar(x2))) => {
                    match x1.cmp(x2) {
                        /* ∧{x} ∨ ∧{x} ≡ ∧{x} */
                        Ordering::Equal => {
                            dbg_log!(DBG_EXTRA2, "∧{{x}} ∨ ∧{{x}} ≡ ∧{{x}}");
                            take_me = TermReductionAction::Move;
                            take_other = TermReductionAction::Skip;
                        },
                        /* (∧{x} ∧ p) ∨ ∧{x} ≡ ∧{x}, (∧{x} ∧ ¬p) ∨ ∧{x} ≡ ∧{x} */
                        Ordering::Less => {
                            dbg_log!(
                                DBG_EXTRA2,
                                "(∧{{x}} ∧ p) ∨ ∧{{x}} ≡ ∧{{x}}, (∧{{x}} ∧ ¬p) ∨ ∧{{x}} ≡ ∧{{x}}"
                            );
                            if !less_strict_than_other {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Ignore;
                                less_strict_than_me = true;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Greater => {
                            dbg_log!(
                                DBG_EXTRA2,
                                "∧{{x}} ∨ (∧{{x}} ∧ p) ≡ ∧{{x}}, ∧{{x}} ∨ (∧{{x}} ∧ ¬p) ≡ ∧{{x}}"
                            );
                            if !less_strict_than_me {
                                take_me = TermReductionAction::Ignore;
                                take_other = TermReductionAction::Skip;
                                less_strict_than_other = true;
                            } else {
                                break 'fsm false;
                            }
                        }
                    }
                }
                (Some(FormulaTerm::Var(x)), Some(FormulaTerm::NegVar(y)))
                    | (Some(FormulaTerm::NegVar(x)), Some(FormulaTerm::Var(y))) =>
                {
                    match x.cmp(y) {
                        /* (p ∧ ∧{x}) ∨ (¬p ∧ ∧{x}) ≡ ∧{x} */
                        Ordering::Equal => {
                            dbg_log!(DBG_EXTRA2, "(p ∧ ∧{{x}}) ∨ (¬p ∧ ∧{{x}}) ≡ ∧{{x}}");
                            /* XXX: If the formula is already less strict, then there must've
                             * been some difference between terms. This would render the 
                             * reduction invalid as it depends on all terms except p and ¬p
                             * being the same. */
                            if !(less_strict_than_me | less_strict_than_other) {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Skip;
                                less_strict_than_me = true;
                                less_strict_than_other = true;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Less => {
                            if !(less_strict_than_me | less_strict_than_other) {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Ignore;
                                less_strict_than_me = true;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Greater => {
                            if !(less_strict_than_me | less_strict_than_other) {
                                take_me = TermReductionAction::Ignore;
                                take_other = TermReductionAction::Skip;
                                less_strict_than_other = true;
                            } else {
                                break 'fsm false;
                            }
                        }
                    }
                },
                (Some(_), None | Some(FormulaTerm::True))
                    | (None | Some(FormulaTerm::True), Some(_)) =>
                {
                    dbg_log!(DBG_EXTRA2, "(∧{{x}} ∧ ⊤) ∨ ∧{{x}} ≡ ∧{{x}}");
                    dbg_log!(DBG_EXTRA2, "  (∧{{x}} ∧ p) ∨ ∧{{x}} ≡ ∧{{x}}, (∧{{x}} ∧ ¬p) ∨ ∧{{x}} ≡ ∧{{x}}");
                    /* (∧{x} ∧ ⊤) ∨ ∧{x} ≡ ∧{x} */
                    /* (∧{x} ∧ p) ∨ ∧{x} ≡ ∧{x}, (∧{x} ∧ ¬p) ∨ ∧{x} ≡ ∧{x} */
                    break 'fsm !less_strict_than_me | less_strict_than_other; /* ! */
                },
                (None, None) => break 'fsm true,
            }

            match take_me {
                TermReductionAction::Move => {
                    dbg_log!(DBG_EXTRA2, "Pushing (left) {:?}", my_yield);
                    if let Some(term) = my_yield {
                        reduced.terms.push(term.clone());
                    }
                    my_yield = my_it.next();
                },
                TermReductionAction::Skip => {
                    dbg_log!(DBG_EXTRA2, "Skipping (left) {:?}", my_yield);
                    my_yield = my_it.next();
                }
                TermReductionAction::Ignore => {
                    dbg_log!(DBG_EXTRA2, "Ignoring (left) {:?}", my_yield);
                },
            }

            match take_other {
                TermReductionAction::Move => {
                    dbg_log!(DBG_EXTRA2, "Pushing (right) {:?}", others_yield);
                    if let Some(term) = others_yield {
                        reduced.terms.push(term.clone());
                    }
                    others_yield = my_it.next();
                },
                TermReductionAction::Skip => {
                    dbg_log!(DBG_EXTRA2, "Skipping (right) {:?}", others_yield);
                    others_yield = others_it.next();
                }
                TermReductionAction::Ignore => {
                    dbg_log!(DBG_EXTRA2, "Ignoring (right) {:?}", others_yield);
                },
            }
        };

        if reductible {
            dbg_log!(DBG_EXTRA2, "Reduction SUCCCESS! Reduced form: {:?}", reduced);
            Some(reduced)
        } else {
            dbg_log!(DBG_EXTRA2, "Reduction FAILURE!");
            None
        }
    }
}

impl<Id> Clone for DNFCube<Id> where Id: Ord + Eq + Clone {
    fn clone(&self) -> Self {
        Self { terms: self.terms.clone() }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DNFForm<Id> where Id: Ord + Eq {
    pub cubes: Vec<DNFCube<Id>>,
}

impl<Id> DNFForm<Id> where Id: Ord + Eq {
    pub fn new() -> Self {
        Self { cubes: Vec::new() }
    }

    /// Returns `true` if the `other` formula contains all the cubes that this formula
    /// has. In other words, if there exisits interpretation that satisfies this formula,
    /// but does not satisfy the other formula, this function will return `false`.
    /// If the function returns `true`, every interpretation that satisfies this formula
    /// will satisfy the other formula.
    pub fn is_subformula_of(&self, other: &Self) -> bool {
        'my_cube_loop: for cube in &self.cubes {
            for other_cube in &other.cubes {
                if cube.is_subcube(other_cube) {
                    continue 'my_cube_loop;
                }
            }
            return false;
        }
        return true;
    }

    pub fn num_cubes(&self) -> usize {
        self.cubes.len()
    }
}

pub trait MergableDNFForm<Id> where
    DNFCube<Id>: std::fmt::Debug,
    FormulaTerm<Id>: std::fmt::Debug,
    Id: Ord + Eq
{
    fn add_cube_opt(self, cube: DNFCube<Id>) -> Self;
    fn add_cube(self, cube: DNFCube<Id>) -> Self;
    fn disjunct_opt(self, other: Self) -> Self;
    fn disjunct(self, other: Self) -> Self;
    fn conjunct_term(self, term: &FormulaTerm<Id>) -> Self;
    fn conjunct_term_with(self, at: usize, term: FormulaTerm<Id>) -> Self;
    fn optimize(self) -> Self;
}

impl<Id> MergableDNFForm<Id> for DNFForm<Id> where
    DNFCube<Id>: std::fmt::Debug,
    FormulaTerm<Id>: std::fmt::Debug,
    Id: Ord + Eq + Clone,
    Self: std::fmt::Debug
{
    /* Complexity: O(terms * cubes) */
    fn add_cube_opt(mut self, cube: DNFCube<Id>) -> Self {
        let mut reduced = false;
        'outer: loop {
            'inner: for (idx, my_cube) in self.cubes.iter_mut().enumerate() {
                match my_cube.try_to_reduce_disjunction(&cube) {
                    Some(reduction) =>{
                        if reduction.is_false_const() {
                            self.cubes.remove(idx);
                            reduced = true;
                            continue 'outer;
                        } else {
                            /* TODO: Mybe there are multiple possibilities.
                             * Which one is the best?
                             */
                            
                            reduced = true;
                            if my_cube.terms != reduction.terms {
                                *my_cube = reduction;
                                continue 'outer;
                            } else {
                                continue 'inner;
                            }
                        }
                    },
                    None => break 'outer,
                }
            }
            break 'outer;
        }
        if !reduced {
            self.cubes.push(cube);
        } 
        self
    }

    fn add_cube(mut self, cube: DNFCube<Id>) -> Self {
        self.cubes.push(cube);
        self
    }

    /* Complexity: pretty bad */
    fn disjunct_opt(self, other: Self) -> Self {
        other.cubes.into_iter()
            .fold(self, |me, cube| me.add_cube_opt(cube))
    }

    fn disjunct(self, other: Self) -> Self {
        other.cubes.into_iter()
            .fold(self, |me, cube| me.add_cube(cube))
    }

    fn conjunct_term(mut self, term: &FormulaTerm<Id>) -> Self {
        for cube in &mut self.cubes {
            cube.add_term(term.clone());
        }
        self
    }

    fn conjunct_term_with(mut self, at: usize, term: FormulaTerm<Id>) -> Self {
        if let Some(cube) = self.cubes.get_mut(at) {
            cube.add_term(term);
        } else {
            /* TODO: Should return Result instead */
            panic!("Cube {} required for conjunction not found", at);
        }
        self
    }

    fn optimize(mut self) -> Self {
        /* Wrapped in DNFForm only for debugging purposes */
        let mut facts = DNFForm { cubes: Vec::new() };

        dbg_log!(DBG_EXTRA1, "+---------------------------------------------------------+");
        dbg_log!(DBG_EXTRA1, ">>>>>>>>>>>> Optimising formula {:?}", &self);
        dbg_log!(DBG_EXTRA1, "+---------------------------------------------------------+");
        
        'outer: loop {
            dbg_log!(DBG_EXTRA1, ">>>>> Form: {:?}, Other facts: {:?}", self, facts);
            let mut reduction = None;
            let facts_iter = self.cubes.iter().chain(facts.cubes.iter());
            'fact_loop: for (fact_idx, fact) in facts_iter.enumerate() {
                for (cube_idx, cube) in self.cubes.iter().enumerate() {
                    match cube.try_to_reduce_disjunction(&fact) {
                        Some(new_cube) => {
                            if new_cube.terms != cube.terms {
                                reduction = Some((new_cube, cube_idx, fact_idx));
                                break 'fact_loop;
                            }
                        },
                        None => (),
                    }
                }
            }
            if let Some((new_cube, at, of)) = reduction {
                #[allow(unused)]
                { /* Literally just debug code */
                    let of_cube = if of < self.cubes.len() {
                        &self.cubes[of]
                    } else {
                        &facts.cubes[of - self.cubes.len()]
                    };
                    
                    dbg_log!(
                        DBG_EXTRA1, ">>>>> {:?}, {:?} => {:?}",
                        self.cubes[at],
                        of_cube,
                        new_cube
                    );
                }

                facts.cubes.push(new_cube.clone());
                self.cubes.push(new_cube);
                let old_cube = self.cubes.swap_remove(at);
                facts.cubes.push(old_cube);
                /* XXX: of > self.cubes.len() when of is a fact outside of the formula */
                if of < self.cubes.len() {
                    self.cubes.remove(of);
                }
            } else {
                break 'outer;
            }
        }

        self
    }
}

impl<Id> std::fmt::Debug for DNFForm<Id> where Id: Ord + Eq + std::fmt::Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.cubes.len() == 0 {
            return write!(f, "DNFForm {{ f(X) = ⊥ }}");
        }
        let cube_str = |c: &DNFCube<Id>| {
            c.terms.iter()
            .map(|term| format!("{:?}", term))
            .tmp_intersperse(" ∧ ".into())
            .fold(String::new(), |s, sym| s + &sym)
        };
        let s = self.cubes.iter()
            .map(|cube| {
                match (self.cubes.len(), cube.len()) {
                    (_, 0) => "⊤".to_string(),
                    (_, 1) | (1, _) => cube_str(cube),
                    _ => format!("({})", cube_str(cube)),
                }
            })
            .tmp_intersperse(" ∨ ".into())
            .fold(String::new(), |s, sym| s + &sym);
        write!(f, "DNFForm {{ f(X) = {} }}", s)
    }
}

impl<Id> Clone for DNFForm<Id> where Id: Ord + Eq + Clone {
    fn clone(&self) -> Self {
        Self { cubes: self.cubes.clone() }
    }
}

/* WARNING: This is slow. */
impl<Id> PartialEq for DNFForm<Id> where Id: Ord + Eq {
    fn eq(&self, other: &Self) -> bool {
        self.is_subformula_of(other)
            && ((self.cubes.len() == other.cubes.len()) || other.is_subformula_of(self))
    }
}
