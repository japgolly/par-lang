#![cfg(test)]

use std::path::PathBuf;

use crate::check;

const LARGE_TEST_STACK_SIZE: usize = 6 * 1024 * 1024;

fn run_with_large_stack<T>(f: impl FnOnce() -> T + Send + 'static) -> T
where
    T: Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("par-large-stack-test".to_string())
        .stack_size(LARGE_TEST_STACK_SIZE)
        .spawn(f)
        .expect("failed to spawn large-stack test thread");
    match handle.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
fn check_all_examples() -> Result<(), String> {
    run_with_large_stack(|| {
        // Check every example package: the small examples share one package,
        // and each big example is its own.
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut packages = vec![examples.join("small")];
        let mut big = std::fs::read_dir(examples.join("big"))
            .map_err(|err| format!("Failed to read big examples: {err}"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("Failed to read big examples: {err}"))?;
        big.retain(|path| path.join("Par.toml").is_file());
        big.sort();
        packages.extend(big);

        for d in packages {
            eprintln!("Checking {:?}", d);
            check(d)?;
        }
        Ok(())
    })
}

#[test]
fn check_playground_examples() -> Result<(), String> {
    run_with_large_stack(|| {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("playground-examples");
        eprintln!("Checking {:?}", d);
        check(d)
    })
}

#[test]
fn test_all_files() -> Result<(), String> {
    run_with_large_stack(|| {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("tests");
        eprintln!("Testing {:?}", d);
        if crate::test_runner::run_tests(
            d,
            None,
            None,
            10_000,
            Some(std::time::Duration::from_secs(60)),
        ) {
            Ok(())
        } else {
            Err("Some tests failed".to_string())
        }
    })
}
