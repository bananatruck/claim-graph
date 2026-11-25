//! Parser for a run log in JSON Lines.
//!
//! See SPEC.md, section "Run log (JSON Lines)", for the input contract.
//! Required fields are checked strictly; extra fields, which real loggers add, are ignored.

use std::collections::HashSet;

use serde::Deserialize;

use crate::draft::{ParseError, is_identifier};

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
    /// The experiment this one replaces, if any.
    pub supersedes: Option<String>,
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
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Experiment {
        id: String,
        name: String,
        supersedes: Option<String>,
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
    Note {},
}

/// Parse a run log. Records may come in any order; references between them are checked
/// after the whole file is read, and errors name the line of the record that refers.
pub fn parse_run_log(input: &str) -> Result<RunLog, ParseError> {
    let mut log = RunLog::default();
    let mut experiment_lines = Vec::new();
    let mut run_lines = Vec::new();
    let mut metric_lines = Vec::new();
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
            Record::Experiment {
                id,
                name,
                supersedes,
            } => {
                check_id(&id, line)?;
                if !experiment_ids.insert(id.clone()) {
                    return Err(fail(format!("experiment `{id}` is defined twice")));
                }
                experiment_lines.push(line);
                log.experiments.push(Experiment {
                    id,
                    name,
                    supersedes,
                });
            }
            Record::Run {
                id,
                experiment,
                seed,
                status,
            } => {
                check_id(&id, line)?;
                if !run_ids.insert(id.clone()) {
                    return Err(fail(format!("run `{id}` is defined twice")));
                }
                run_lines.push(line);
                log.runs.push(Run {
                    id,
                    experiment,
                    seed,
                    status,
                });
            }
            Record::Metric { run, name, value } => {
                metric_lines.push(line);
                log.metrics.push(Metric { run, name, value });
            }
            Record::Note {} => {}
        }
    }

    for (experiment, &line) in log.experiments.iter().zip(&experiment_lines) {
        let Some(older) = &experiment.supersedes else {
            continue;
        };
        if older == &experiment.id || !experiment_ids.contains(older) {
            return Err(ParseError {
                line,
                message: format!(
                    "experiment `{}` supersedes `{older}`, which is not another defined experiment",
                    experiment.id
                ),
            });
        }
    }
    if let Some(line) = supersedes_cycle(&log.experiments, &experiment_lines) {
        return Err(ParseError {
            line,
            message: "experiments supersede each other in a cycle".to_string(),
        });
    }
    for (run, &line) in log.runs.iter().zip(&run_lines) {
        if !experiment_ids.contains(&run.experiment) {
            return Err(ParseError {
                line,
                message: format!(
                    "run `{}` names experiment `{}`, which is not defined",
                    run.id, run.experiment
                ),
            });
        }
    }
    for (metric, &line) in log.metrics.iter().zip(&metric_lines) {
        if !run_ids.contains(&metric.run) {
            return Err(ParseError {
                line,
                message: format!(
                    "metric `{}` names run `{}`, which is not defined",
                    metric.name, metric.run
                ),
            });
        }
    }
    Ok(log)
}

/// Run and experiment IDs must be ones a draft can write as `[run:ID]` or `[exp:ID]`.
fn check_id(id: &str, line: usize) -> Result<(), ParseError> {
    if is_identifier(id) {
        Ok(())
    } else {
        Err(ParseError {
            line,
            message: format!(
                "ID `{id}` must be letters, digits, `-`, `_`, `.` or `:`, so a draft can reference it"
            ),
        })
    }
}

/// The line of an experiment on a `supersedes` cycle, if there is one.
/// Each experiment supersedes at most one other, so following the links from any start
/// either ends or comes back to an experiment already seen on that walk. Starts are tried
/// in file order, and the line reported is that of the first experiment a walk meets
/// twice. A walk that ended without a cycle marks its experiments done, and a later walk
/// stops at them, so each experiment is visited once and the check is linear.
fn supersedes_cycle(experiments: &[Experiment], lines: &[usize]) -> Option<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Visit {
        New,
        OnWalk,
        Done,
    }
    let mut index = std::collections::HashMap::new();
    for (i, experiment) in experiments.iter().enumerate() {
        index.entry(experiment.id.as_str()).or_insert(i);
    }
    let mut visit = vec![Visit::New; experiments.len()];
    for start in 0..experiments.len() {
        let mut walk = Vec::new();
        let mut current = Some(start);
        while let Some(i) = current {
            match visit[i] {
                Visit::Done => break,
                Visit::OnWalk => return Some(lines[i]),
                Visit::New => {}
            }
            visit[i] = Visit::OnWalk;
            walk.push(i);
            current = experiments[i]
                .supersedes
                .as_deref()
                .and_then(|id| index.get(id).copied());
        }
        for i in walk {
            visit[i] = Visit::Done;
        }
    }
    None
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
                name: "first".into(),
                supersedes: None,
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
    fn unknown_fields_are_ignored() {
        let log = parse_run_log(
            "{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\",\"git_sha\":\"abc\"}\n",
        )
        .unwrap();
        assert_eq!(log.experiments[0].id, "E1");
    }

    #[test]
    fn records_may_come_in_any_order() {
        let input = "{\"kind\":\"metric\",\"run\":\"r\",\"name\":\"m\",\"value\":1.0}\n{\"kind\":\"run\",\"id\":\"r\",\"experiment\":\"E1\",\"status\":\"completed\"}\n{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n";
        let log = parse_run_log(input).unwrap();
        assert_eq!(
            (log.experiments.len(), log.runs.len(), log.metrics.len()),
            (1, 1, 1)
        );
    }

    #[test]
    fn supersedes_links_two_experiments() {
        let input = "{\"kind\":\"experiment\",\"id\":\"E2\",\"name\":\"b\",\"supersedes\":\"E1\"}\n{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n";
        let log = parse_run_log(input).unwrap();
        assert_eq!(log.experiments[0].supersedes.as_deref(), Some("E1"));
        assert_eq!(log.experiments[1].supersedes, None);
    }

    #[test]
    fn supersedes_must_name_another_defined_experiment() {
        assert_eq!(
            error_line(
                "{\"kind\":\"experiment\",\"id\":\"E2\",\"name\":\"b\",\"supersedes\":\"E9\"}\n"
            ),
            1
        );
        assert_eq!(
            error_line(
                "{\"kind\":\"experiment\",\"id\":\"E2\",\"name\":\"b\",\"supersedes\":\"E2\"}\n"
            ),
            1
        );
    }

    #[test]
    fn a_supersedes_cycle_is_an_error() {
        let input = "{\"kind\":\"experiment\",\"id\":\"E12\",\"name\":\"c\",\"supersedes\":\"E11\"}\n{\"kind\":\"experiment\",\"id\":\"E11\",\"name\":\"b\",\"supersedes\":\"E10\"}\n{\"kind\":\"experiment\",\"id\":\"E10\",\"name\":\"a\",\"supersedes\":\"E12\"}\n";
        let err = parse_run_log(input).unwrap_err();
        assert!(err.message.contains("cycle"), "{}", err.message);
        assert!((1..=3).contains(&err.line));
    }

    /// The line reported for each shape of `supersedes` links: the first experiment found
    /// twice on a walk, trying starts in file order.
    #[test]
    fn a_supersedes_cycle_reports_where_the_first_walk_meets_itself() {
        let experiment = |id: &str, supersedes: Option<&str>| match supersedes {
            Some(s) => format!(
                "{{\"kind\":\"experiment\",\"id\":\"{id}\",\"name\":\"x\",\"supersedes\":\"{s}\"}}\n"
            ),
            None => format!("{{\"kind\":\"experiment\",\"id\":\"{id}\",\"name\":\"x\"}}\n"),
        };
        for (links, expected) in [
            (
                &[("A", Some("B")), ("B", Some("C")), ("C", Some("A"))][..],
                Some(1),
            ),
            (
                &[("A", Some("B")), ("B", Some("C")), ("C", Some("B"))],
                Some(2),
            ),
            (&[("A", None), ("B", Some("B"))], Some(2)),
            (
                &[
                    ("A", Some("B")),
                    ("B", Some("A")),
                    ("C", Some("D")),
                    ("D", Some("C")),
                ],
                Some(1),
            ),
            (
                &[
                    ("A", Some("B")),
                    ("B", None),
                    ("C", Some("D")),
                    ("D", Some("C")),
                ],
                Some(3),
            ),
            (&[("A", Some("B")), ("B", Some("C")), ("C", None)], None),
        ] {
            let input: String = links.iter().map(|&(id, s)| experiment(id, s)).collect();
            let line = parse_run_log(&input).err().map(|e| e.line);
            assert_eq!(line, expected, "{links:?}");
        }
    }

    /// A long chain is checked in time linear in its length.
    #[test]
    fn a_long_supersedes_chain_is_checked_quickly() {
        let n = 20_000;
        let mut input = String::from("{\"kind\":\"experiment\",\"id\":\"E0\",\"name\":\"x\"}\n");
        for i in 1..n {
            input.push_str(&format!(
                "{{\"kind\":\"experiment\",\"id\":\"E{i}\",\"name\":\"x\",\"supersedes\":\"E{}\"}}\n",
                i - 1
            ));
        }
        assert_eq!(parse_run_log(&input).unwrap().experiments.len(), n);
        input = input.replacen(
            "\"id\":\"E0\",\"name\":\"x\"",
            &format!("\"id\":\"E0\",\"name\":\"x\",\"supersedes\":\"E{}\"", n - 1),
            1,
        );
        assert!(parse_run_log(&input).is_err());
    }

    #[test]
    fn ids_the_draft_cannot_reference_are_an_error() {
        assert_eq!(
            error_line("{\"kind\":\"experiment\",\"id\":\"exp/1\",\"name\":\"slash\"}\n"),
            1
        );
        assert_eq!(
            error_line(
                "{\"kind\":\"experiment\",\"id\":\"E1\",\"name\":\"a\"}\n{\"kind\":\"run\",\"id\":\"\",\"experiment\":\"E1\",\"status\":\"completed\"}\n"
            ),
            2
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
        let input =
            "{\"kind\":\"run\",\"id\":\"r\",\"experiment\":\"E9\",\"status\":\"completed\"}\n";
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
        assert_eq!(log.runs.len(), 4);
        assert_eq!(log.metrics.len(), 3);
    }

    #[test]
    fn the_messy_synthetic_run_log_parses() {
        let log = parse_run_log(include_str!("../data/synthetic/messy/runs.jsonl")).unwrap();
        assert_eq!(log.experiments.len(), 3);
        assert_eq!(log.runs.len(), 5);
        assert_eq!(log.metrics.len(), 6);
        assert_eq!(log.experiments[0].supersedes.as_deref(), Some("E11"));
    }
}
