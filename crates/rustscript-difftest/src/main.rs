#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use rustscript_difftest::{
    RustcOracle, replay_source, run_case, write_failure_artifact, write_success_artifact,
};

struct Arguments {
    seed: u64,
    cases: usize,
    case: Option<usize>,
    replay: Option<PathBuf>,
    rustc: Option<PathBuf>,
    keep_all: bool,
}

fn main() -> ExitCode {
    match parse_arguments(std::env::args_os().skip(1).collect()).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustscript-difftest: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Arguments) -> Result<(), String> {
    let oracle = RustcOracle::discover(arguments.rustc);
    let artifact_root = PathBuf::from("artifacts/differential");
    if let Some(path) = arguments.replay {
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let source = String::from_utf8(bytes)
            .map_err(|_| format!("{} is not valid UTF-8", path.display()))?;
        match replay_source(&source, &oracle) {
            Ok(success) => {
                if arguments.keep_all {
                    write_success_artifact(&success, &artifact_root)
                        .map_err(|error| error.to_string())?;
                }
                println!("replay passed: {} steps", success.steps);
                Ok(())
            }
            Err(failure) => report_failure(&failure, &artifact_root),
        }
    } else {
        let cases: Box<dyn Iterator<Item = usize>> = match arguments.case {
            Some(case) => Box::new(std::iter::once(case)),
            None => Box::new(0..arguments.cases),
        };
        let mut completed = 0usize;
        for case_index in cases {
            match run_case(arguments.seed, case_index, &oracle) {
                Ok(success) => {
                    if arguments.keep_all {
                        write_success_artifact(&success, &artifact_root)
                            .map_err(|error| error.to_string())?;
                    }
                    completed += 1;
                }
                Err(failure) => return report_failure(&failure, &artifact_root),
            }
        }
        println!(
            "differential run passed: seed {}, {} case(s)",
            arguments.seed, completed
        );
        Ok(())
    }
}

fn report_failure(
    failure: &rustscript_difftest::DiffFailure,
    root: &std::path::Path,
) -> Result<(), String> {
    let directory = write_failure_artifact(failure, root)
        .map_err(|error| format!("failed to write artifact: {error}"))?;
    Err(format!(
        "seed {} case {} failed ({:?}): {}; artifact: {}",
        failure.seed,
        failure.case_index,
        failure.kind,
        failure.reason,
        directory.display()
    ))
}

fn parse_arguments(arguments: Vec<std::ffi::OsString>) -> Result<Arguments, String> {
    let mut parsed = Arguments {
        seed: 1,
        cases: 1000,
        case: None,
        replay: None,
        rustc: None,
        keep_all: false,
    };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument
            .to_str()
            .ok_or_else(|| "arguments must be valid UTF-8".to_owned())?;
        match argument {
            "--seed" => parsed.seed = parse_value(arguments.next(), "--seed")?,
            "--cases" => parsed.cases = parse_value(arguments.next(), "--cases")?,
            "--case" => parsed.case = Some(parse_value(arguments.next(), "--case")?),
            "--replay" => {
                parsed.replay = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--replay requires a path".to_owned())?,
                ))
            }
            "--rustc" => {
                parsed.rustc = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--rustc requires a path".to_owned())?,
                ))
            }
            "--keep-all" => parsed.keep_all = true,
            _ => return Err(format!("unknown argument `{argument}`")),
        }
    }
    if parsed.replay.is_some() && parsed.case.is_some() {
        return Err("--replay and --case are mutually exclusive".into());
    }
    Ok(parsed)
}

fn parse_value<T: std::str::FromStr>(
    value: Option<std::ffi::OsString>,
    flag: &str,
) -> Result<T, String> {
    value
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| format!("{flag} requires a value"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runner_modes() {
        let arguments = parse_arguments(
            ["--seed", "7", "--case", "3", "--keep-all"]
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .unwrap();
        assert_eq!(arguments.seed, 7);
        assert_eq!(arguments.case, Some(3));
        assert!(arguments.keep_all);
    }
}
