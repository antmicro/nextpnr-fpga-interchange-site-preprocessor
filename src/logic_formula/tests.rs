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

#[test]
fn test_variable_comparison() {
    use FormulaTerm::*;
    use TestVar::*;

    assert!(False < Var(A));
    assert!(Var(A) < NegVar(A));
    assert!(NegVar(A) < Var(B));
    assert!(Var(B) < True);
}

#[test]
fn test_variable_sorting() {
    use FormulaTerm::*;
    use TestVar::*;

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
    use FormulaTerm::*;
    use TestVar::*;

    /* f1(X) = x */
    let form1 = DNFForm::new()
        .merge_cube(DNFCube { terms: vec![Var(X)] });
    
        
    /* f2(X) = x */
    let form2 = form1.clone();

    let expected = form1.clone();

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
    let expected = DNFForm::new().merge_cube(DNFCube { terms: vec![] });

    assert_eq!(result, expected);
}

#[test]
fn test_cube_failed_merging_simple() {
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
