use super::*;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
enum TestVar {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, W, V, X, Y, Z,
}

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

use FormulaTerm::*;
use TestVar::*;

#[test]
fn test_variable_comparison() {
    assert!(False < Var(A));
    assert!(Var(A) < NegVar(A));
    assert!(NegVar(A) < Var(B));
    assert!(Var(B) < True);
}

#[test]
fn test_variable_sorting() {
    let mut cube = DNFCube::new();

    cube.add_term(Var(C));
    cube.add_term(Var(A));
    cube.add_term(NegVar(B));
    cube.add_term(True);

    let expected = DNFCube {
        terms: vec![Var(A), NegVar(B), Var(C), True],
    };

    assert_eq!(cube, expected);
}

#[test]
fn test_reduction_of_two_same_cubes() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![Var(X)] });
    
    let form2 = form1.clone();

    let expected = form1.clone();

    let result = form1.disjunct(form2);
    assert_eq!(result, expected);
}

#[test]
fn test_reduction_of_two_different_cubes() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X)] });
    
    let form2 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![Var(X)] });

    let result = form1.disjunct(form2);
    let expected = DNFForm::new().add_cube(DNFCube { terms: vec![] });

    assert_eq!(result, expected);
}

#[test]
fn test_cube_failed_merging_simple() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X)] })
        .add_cube(DNFCube { terms: vec![NegVar(Y), NegVar(Z)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(X)] });
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y), NegVar(Z)] });

    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_complementaries_left() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] })
        .add_cube(DNFCube { terms: vec![Var(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_complementaries_right() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![Var(X), NegVar(Y)] })
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_overspecification_left() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] })
        .add_cube(DNFCube { terms: vec![NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_overspecification_right() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(Y)] })
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(Y)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_no_reduction_of_complementaries_left() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] })
        .add_cube(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] });

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });
    expected.cubes.push(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_no_reduction_of_same() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] })
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });

    let mut expected = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_reduction_of_same_2() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] })
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });

    let mut expected = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_double_reduction_of_complementaries() {

    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] }) /* 1 */
        .add_cube(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] })       /* 2 */
        .add_cube(DNFCube { terms: vec![NegVar(X), Var(Y), Var(Z)] });   /* 3 */

    /* This test checks if the system is capale of re-using the same equation
     * to perform multiple reductions over conjunction groups.
     * 
     * Below is the expected reasoning:
     * 
     *        .---G[1]----.   .--G[2]---.   .---G[3]---.
     * f(X) = (¬x ∧ ¬y ∧ z) ∨ (x ∧ y ∧ z) ∨ (¬x ∧ y ∧ z)
     * ------------------------------------
     * 4. G[1] ∨ G[3] ≡ ¬x ∧ z
     * 5. G[2] ∨ G[3] => ¬y ∧ z
     * 
     * ERGO:
     *    f(X) = (¬x ∧ z) ∨ (¬y ∧ z)
     * ------------------------------------
     */

    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![NegVar(X), Var(Z)] });
    expected.cubes.push(DNFCube { terms: vec![Var(Y), Var(Z)] });
    
    assert_eq!(form1, expected);
}

#[test]
fn test_quadruple_reduction_of_complementaries() {
    let form1 = DNFForm::new()
        .add_cube(DNFCube { terms: vec![NegVar(X), NegVar(Y), Var(Z)] }) /* 1 */
        .add_cube(DNFCube { terms: vec![Var(X), Var(Y), Var(Z)] })       /* 2 */
        .add_cube(DNFCube { terms: vec![NegVar(X), Var(Y), Var(Z)] })    /* 3 */
        .add_cube(DNFCube { terms: vec![Var(X), NegVar(Y), Var(Z)] });   /* 4 */
    
    /* This test checks if the system is capale of re-using the same equation
     * to perform multiple reductions over conjunction groups.
     * 
     * Below is the expected reasoning:
     * 
     *        .---G[1]----.   .--G[2]---.   .---G[3]---.   .---G[4]---.
     * f(X) = (¬x ∧ ¬y ∧ z) ∨ (x ∧ y ∧ z) ∨ (¬x ∧ y ∧ z) ∨ (x ∧ ¬y ∧ z)
     * ------------------------------------
     * 5. G[1] ∨ G[3] ≡ ¬x ∧ z
     * 6. G[2] ∨ G[3] ≡ y ∧ z
     * 7. G[1] ∨ G[4] ≡ ¬y ∧ z
     * 8. G[2] ∨ G[4] ≡ x ∧ z
     * 
     * ERGO:
     *    f(X) = z
     * ------------------------------------
     */


    let mut expected = DNFForm::new();
    expected.cubes.push(DNFCube { terms: vec![Var(Z)] });
    
    assert_eq!(form1, expected);
}

