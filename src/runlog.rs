//! Strict parser for a run log in JSON Lines.
//!
//! See SPEC.md, section "Run log (JSON Lines)", for the input contract.

use std::collections::HashSet;

use serde::Deserialize;

use crate::draft::ParseError;

/// Everything a run log records, in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunLog {
    pub experiments: Vec<Experiment>,
    pub runs: Vec<Run>,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: String,
    pub experiment: String,
    pub seed: Option<i64>,
    /// Free text; only `completed` counts as support.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub run: String,
    pub name: String,
    pub value: f64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Record {
    Experiment {
        id: String,
        name: String,
    },
    Run {
        id: String,
        experiment: String,
        seed: Option<i64>,
        status: String,
    },
    Metric {
        run: String,
        name: String,
        value: f64,
    },
    Note {
        #[allow(dead_code)]
        text: String,
    },
}

/// Parse a run log. Runs must follow their experiment and metrics must follow their run.
pub fn parse_run_log(input: &str) -> Result<RunLog, ParseError> {
    let mut log = RunLog::default();
    let mut experiment_ids = HashSet::new();
    let mut run_ids = HashSet::new();
    for (index, raw) in input.lines().enumerate() {
        let line = index + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let fail = |message: String| ParseError { line, message };
        let record: Record = serde_json::from_str(raw).map_err(|e| fail(e.to_string()))?;
        match record {
            Record::Experiment { id, name } => {
                if !experiment_ids.insert(id.clone()) {
                    return Err(fail(format!("experiment `{id}` is defined twice")));
                }
                log.experiments.push(Experiment { id, name });
            }
            Record::Run {
                id,
                experiment,
                seed,
                status,
            } => {
                if !experiment_ids.contains(&experiment) {
                    return Err(fail(format!(
                        "run `{id}` names experiment `{experiment}`, which is not defined above it"
                    )));
                }
                if !run_ids.insert(id.clone()) {
                    return Err(fail(format!("run `{id}` is defined twice")));
                }
                log.runs.push(Run {
                    id,
                    experiment,
                    seed,
                    status,
                });
            }
            Record::Metric { run, name, value } => {
                if !run_ids.contains(&run) {
                    return Err(fail(format!(
                        "metric `{name}` names run `{run}`, which is not defined above it"
                    )));
                }
                log.metrics.push(Metric { run, name, value });
            }
            Record::Note { .. } => {}
        }
    }
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"kind":"experiment","id":"E1","name":"first"}
{"kind":"run","id":"r-1","experiment":"E1","seed":7,"status":"completed"}

{"kind":"metric","run":"r-1","name":"success_rate","value":0.5}
{"kind":"run","id":"r-2","experiment":"E1","status":"failed"}
{"kind":"note","text":"synthetic"}
"#;

    #[test]
    fn reads_each_kind_and_skips_blank_lines_and_notes() {
        let log = parse_run_log(GOOD).unwrap();
        assert_eq!(
            log.experiments,
            [Experiment {
                id: "E1".into(),
                name: "first".into()
            }]
        );
        assert_eq!(log.runs.len(), 2);
        assert_eq!(log.runs[0].seed, Some(7));
        assert_eq!(log.runs[1].seed, None);
        assert_eq!(log.runs[1].status, "failed");
        assert_eq!(
            log.metrics,
            [Metric {
                run: "r-1".into(),
                name: "success_rate".into(),
                value: 0.5
            }]
        );
    }

    fn error_line(input: &str) -> usize {
        parse_run_log(input).unwrap_err().line
    }

    #[test]
    fn an_unknown_kind_is_an_error() {
        assert_eq!(error_line("{\"kind\":\"plot\",\"id\":\"x\"}\n"), 1);
    }

    #[test]
    fn an_unknown_field_is_an_error() {
        assert_eq!(
            error_line("{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\",\"nmae\":\"b\"}\n"),
            1
        );
    }

    #[test]
    fn invalid_json_reports_its_line() {
        assert_eq!(
            error_line("{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n{not json\n"),
            2
        );
    }

    #[test]
    fn a_duplicate_experiment_id_is_an_error() {
        let input = "{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"b\"}\n";
        assert_eq!(error_line(input), 2);
    }

    #[test]
    fn a_duplicate_run_id_is_an_error() {
        let input = "{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n{\"kind\":\"run\",\"id\":\"r\",\"experiment\":\"E1\",\"status\":\"completed\"}\n{\"kind\":\"run\",\"id\":\"r\",\"experiment\":\"E1\",\"status\":\"completed\"}\n";
        assert_eq!(error_line(input), 3);
    }

    #[test]
    fn a_run_naming_an_unknown_experiment_is_an_error() {
        let input = "{\"kind\":\"run\",\"id\":\"r\",\"experiment\":\"E9\",\"status\":\"completed\"}\n";
        assert_eq!(error_line(input), 1);
    }

    #[test]
    fn a_metric_naming_an_unknown_run_is_an_error() {
        let input = "{\"kind\":\"metric\",\"run\":\"r9\",\"name\":\"m\",\"value\":1.0}\n";
        assert_eq!(error_line(input), 1);
    }

    #[test]
    fn the_synthetic_run_log_parses() {
        let log = parse_run_log(include_str!("../data/synthetic/runs.jsonl")).unwrap();
        assert_eq!(log.experiments.len(), 3);
        assert_eq!(log.runs.len(), 3);
        assert_eq!(log.metrics.len(), 2);
    }
}
