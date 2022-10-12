/* The representations  might be suboptimal at the moment
 * What's important is the functionality. Optimisations can come later.
 */

use std::cmp::Ordering;

mod intersperse;
use self::intersperse::*;

enum FormulaTerm<Id> where Id: Ord + Eq {
    Var(Id),
    NegVar(Id),
    True,
    False,
}

impl<Id> FormulaTerm<Id> where Id: Ord + Eq {
    /* Check if term is negation of the other term */
    fn neg_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Var(a), Self::NegVar(b)) => a == b,
            (Self::NegVar(a), Self::Var(b)) => a == b,
            (Self::True, Self::False) | (Self::False, Self::True) => true,
            _ => false
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

#[derive(PartialEq, Eq)]
struct DNFCube<Id> where Id: Ord + Eq {
    terms: Vec<FormulaTerm<Id>>
}

/* Represents a conjunction group ("cube") in DNF boolean formula */
impl<Id> DNFCube<Id> where Id: Ord + Eq {
    fn new() -> Self {
        Self { terms: Vec::new() }
    }

    fn len(&self) -> usize {
        self.terms.len()
    }

    fn is_true_const(&self) -> bool {
        self.terms.iter().find(|term| {
            if let FormulaTerm::True = term {
                false
            } else {
                true
            }
        }).is_none()
    }

    fn is_false_const(&self) -> bool {
        self.terms.contains(&FormulaTerm::False)
    }

    fn add_term(&mut self, term: FormulaTerm<Id>) {
        /* Could be done faster in terms of time complexity */
        let idx = {
            let mut my_term_idx = 0;
            loop {
                if my_term_idx == self.terms.len() { break my_term_idx; }
                let my_term = &self.terms[my_term_idx];
                if &term > my_term { break my_term_idx; }
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
}

trait ReductibleDNFCube<Id> where Self: Sized {
    /* Attempts to reduce disjunction of two cubes into a single cube */
    fn try_to_reduce_disjunction(&self, other: &Self) -> Option<Self>;
}

enum TermReductionAction {
    Move,   /* Moves the term to the reduced form */
    Skip,   /* Skips the term */
    Ignore, /* Ignores the term in this iteration. To be checked in the next one */
}

impl<Id> ReductibleDNFCube<Id> for DNFCube<Id> where Id: Ord + Eq + Clone {
    fn try_to_reduce_disjunction(&self, other: &Self) -> Option<Self> {
        /* Reduced cube in construction */
        let mut reduced = DNFCube::new();

        /* Becomes true, when the reduced cube prefix turns out to be less
         * strict than any of the input cubes prefixes */
        let mut less_strict = false;
        
        /* Note: terms in cubes must be sorted! */
        let mut my_it = self.terms.iter().peekable();
        let mut others_it = other.terms.iter().peekable();
        
        /* Traverse sorted cubes to check for same variables, their negations
         * and constants */
        let mut my_yield = my_it.next();
        let mut others_yield = others_it.next();
        let reductible: bool = 'fsm: loop {
            let mut take_me = TermReductionAction::Move;
            let mut take_other = TermReductionAction::Move;

            /* Perform reductions */
            match (my_yield, others_yield) {
                /* ⊥ ∧ ∧{x} ∨ ∧{y} ≡ ∧{y} */
                (Some(FormulaTerm::False), _) => {
                    return Some(other.clone());
                },
                (_, Some(FormulaTerm::False)) => {
                    return Some(self.clone());
                }
                /* ⊤ ∨ ⊤ ≡ ⊤ */
                (Some(FormulaTerm::True), Some(FormulaTerm::True)) => {
                    take_other = TermReductionAction::Move;
                    take_me = TermReductionAction::Skip;
                },
                (Some(t1 @ FormulaTerm::Var(x1)), t2 @ Some(FormulaTerm::Var(x2)))
                | (Some(t1 @ FormulaTerm::NegVar(x1)), t2 @ Some(FormulaTerm::NegVar(x2))) => {
                    match x1.cmp(x2) {
                        /* ∧{x} ∨ ∧{x} ≡ ∧{x} */
                        Ordering::Equal => {
                            take_me = TermReductionAction::Move;
                            take_other = TermReductionAction::Skip;
                        },
                        /* (∧{x} ∧ p) ∨ ∧{x} ≡ ∧{x}, (∧{x} ∧ ¬p) ∨ ∧{x} ≡ ∧{x} */
                        Ordering::Less => {
                            if !less_strict {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Ignore;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Greater => {
                            if !less_strict {
                                take_me = TermReductionAction::Ignore;
                                take_other = TermReductionAction::Skip;
                            } else {
                                break 'fsm false;
                            }
                        }
                    }
                }
                (Some(FormulaTerm::Var(x)), Some(FormulaTerm::NegVar(notx)))
                    | (Some(FormulaTerm::NegVar(notx)), Some(FormulaTerm::Var(x))) =>
                {
                    match x.cmp(notx) {
                        /* (p ∧ ∧{x}) ∨ (¬p ∧ ∧{x}) ≡ ∧{x} */
                        Ordering::Equal => {
                            /* XXX: If the formula is already less strict, then there must've
                             * been some difference between terms. This would render the 
                             * reduction invalid as it depends on all terms except p and ¬p
                             * being the same. */
                            if !less_strict {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Skip;
                                less_strict = true;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Less => {
                            if !less_strict {
                                take_me = TermReductionAction::Skip;
                                take_other = TermReductionAction::Ignore;
                            } else {
                                break 'fsm false;
                            }
                        },
                        Ordering::Greater => {
                            if !less_strict {
                                take_me = TermReductionAction::Ignore;
                                take_other = TermReductionAction::Skip;
                            } else {
                                break 'fsm false;
                            }
                        }
                    }
                    
                    if x == notx && !less_strict {
                        take_me = TermReductionAction::Skip;
                        take_other = TermReductionAction::Skip;
                        less_strict = true;
                    }
                },
                (Some(_), None | Some(FormulaTerm::True))
                    | (None | Some(FormulaTerm::True), Some(_)) =>
                {
                    /* (∧{x} ∧ ⊤) ∨ ∧{x} ≡ ∧{x} */
                    /* (∧{x} ∧ p) ∨ ∧{x} ≡ ∧{x}, (∧{x} ∧ ¬p) ∨ ∧{x} ≡ ∧{x} */
                    break 'fsm !less_strict;
                },
                (None, None) => break 'fsm true,
            }

            match take_me {
                TermReductionAction::Move => {
                    if let Some(term) = my_yield {
                        reduced.terms.push(term.clone());
                    }
                    my_yield = my_it.next();
                },
                TermReductionAction::Skip => my_yield = my_it.next(),
                TermReductionAction::Ignore => (),
            }

            match take_other {
                TermReductionAction::Move => {
                    if let Some(term) = others_yield {
                        reduced.terms.push(term.clone());
                    }
                    others_yield = my_it.next();
                },
                TermReductionAction::Skip => others_yield = others_it.next(),
                TermReductionAction::Ignore => (),
            }
        };

        if reductible {
            Some(reduced)
        } else {
            None
        }
    }
}

impl<Id> std::fmt::Debug for DNFCube<Id> where Id: Ord + Eq + std::fmt::Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.terms.iter()
            .map(|term| format!("{:?}", term))
            .tmp_intersperse(" ∧ ".into())
            .fold(String::new(), |s, sym| s + &sym);
        write!(f, "{}", s)
    }
}

impl<Id> Clone for DNFCube<Id> where Id: Ord + Eq + Clone {
    fn clone(&self) -> Self {
        Self { terms: self.terms.clone() }
    }
}

struct DNFForm<Id> where Id: Ord + Eq {
    cubes: Vec<DNFCube<Id>>,
}

impl<Id> DNFForm<Id> where Id: Ord + Eq {
    fn new() -> Self {
        Self { cubes: Vec::new() }
    }
}

trait MergableDNFForm<Id> where Id: Ord + Eq {
    fn merge_cube(self, cube: DNFCube<Id>) -> Self;
    fn merge(self, other: Self) -> Self;
}

impl<Id> MergableDNFForm<Id> for DNFForm<Id> where Id: Ord + Eq + Clone {
    fn merge_cube(mut self, cube: DNFCube<Id>) -> Self {
        for (idx, my_cube) in self.cubes.iter_mut().enumerate() {
            match my_cube.try_to_reduce_disjunction(&cube) {
                Some(reduction) =>{
                    if reduction.is_false_const() {
                        self.cubes.remove(idx);
                        return self;
                    } else {
                        /* TODO: Mybe there are multiple possibilities.
                         * Which one is the best?
                         */
                        *my_cube = reduction;
                        return self;
                    }
                },
                None => ()
            }
        }
        self.cubes.push(cube);
        self
    }

    fn merge(self, other: Self) -> Self {
        other.cubes.into_iter()
            .fold(self, |me, cube| me.merge_cube(cube))
    }
}

impl<Id> std::fmt::Debug for DNFForm<Id> where Id: Ord + Eq + std::fmt::Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.cubes.len() == 0 {
            return write!(f, "DNFForm {{ f(X) = ⊥ }}");
        }
        let s = self.cubes.iter()
            .map(|cube| {
                match (self.cubes.len(), cube.len()) {
                    (_, 0) => "⊤".to_string(),
                    (_, 1) | (1, _) => format!("{:?}", cube),
                    _ => format!("({:?})", cube),
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
        for cube in &self.cubes {
            for other_cube in &other.cubes {
                if cube == other_cube { continue; }
            }
            return false;
        }
        return true;
    }
}

/* ------------------------ TESTS ------------------------ */

#[cfg(test)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
enum TestVar {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, W, V, X, Y, Z,
}

#[cfg(test)]
impl std::fmt::Debug for TestVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use TestVar::*;
        let ch = match self {
            A => 'a',
            B => 'b',
            C => 'c',
            D => 'd',
            E => 'e',
            F => 'f',
            G => 'g',
            H => 'h',
            I => 'i',
            J => 'j',
            K => 'k',
            L => 'l',
            M => 'm',
            N => 'n',
            O => 'o',
            P => 'p',
            Q => 'q',
            R => 'r',
            S => 's',
            T => 't',
            U => 'u',
            W => 'w',
            V => 'v',
            X => 'x',
            Y => 'y',
            Z => 'z',
        };
        write!(f, "{}", ch)
    }
}

#[test]
fn test_reduction_of_two_same_cubes() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![Var(X)] });
    
        
    /* f2(X) = x */
    let form2 = form1.clone();

    let mut expected = form1.clone();

    let result = form1.merge(form2);
    assert_eq!(result, expected);
}

#[test]
fn test_reduction_of_two_different_cubes() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(X)] });

    /* f2(X) = x */
    let form2 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![Var(X)] });

    let result = form1.merge(form2);

    assert_eq!(result, DNFForm::new().merge_cube(DNFCube { terms: vec![] }));
}

#[test]
fn test_cube_merging() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(X)] })
        .merge_cube(DNFCube { terms: vec![NegVar(Y), NegVar(Z)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(X)] });
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y), NegVar(Z)] });

    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_complementaries_left() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] })
        .merge_cube(DNFCube { terms: vec![Var(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_complementaries_right() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![Var(X), NegVar(Y)] })
        .merge_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_overspecification_left() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] })
        .merge_cube(DNFCube { terms: vec![NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_overspecification_right() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(Y)] })
        .merge_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_no_reduction_of_complementaries_left() {
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] })
        .merge_cube(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });
    expected.cubes.push(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] });
    
    assert_eq!(form1, expected);
}
