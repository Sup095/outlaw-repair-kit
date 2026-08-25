//! Turning findings into an explanation.
//!
//! The order of operations is the important part:
//!
//! 1. Every finding is looked up in the runbook library. A match produces a
//!    deterministic answer -- same input, same output, no model, no network,
//!    no cost.
//! 2. Only what is left over is sent to a model, along with the full set of
//!    findings for cross-cutting correlation.
//! 3. If no model is available, the runbook answers still stand. The tool is
//!    useful with the AI layer switched off entirely, which is the property
//!    that keeps the AI layer honest.
//!
//! What the model receives is the structured findings the probes already
//! produced -- titles, severities, and captured evidence. It does not get
//! access to the machine, and it cannot ask for more.

use std::collections::BTreeSet;

use ork_core::finding::{Finding, Severity};
use ork_core::scan::ScanReport;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::client::ModelClient;
use crate::router::Routing;
use crate::runbook::{CandidateFix, Invasiveness, RunbookLibrary};

/// Where an explanation came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum AnalysisSource {
    /// A known problem with a written-down answer.
    Runbook { entry_id: String },
    /// Reasoned about by a model, because nothing in the library matched.
    Model { model: String },
    /// Nothing matched and no model was available. Said plainly rather than
    /// dressed up as an answer.
    Unexplained,
}

/// One finding, with whatever explanation could be found for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysedFinding {
    pub finding_id: String,
    pub subject: Option<String>,
    pub title: String,
    pub severity: Severity,
    pub source: AnalysisSource,
    pub explanation: String,
    pub fixes: Vec<CandidateFix>,
}

/// The result of analysing a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// What the findings mean taken together, when a model was available to
    /// look across them. Correlation is the thing a model genuinely adds over
    /// a lookup table.
    pub correlation: Option<String>,
    pub items: Vec<AnalysedFinding>,
    /// Which model answered, if any.
    pub model: Option<String>,
    /// How many findings the runbook library answered without a model.
    pub answered_by_runbook: usize,
}

impl Analysis {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Findings nobody could explain.
    pub fn unexplained(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.source == AnalysisSource::Unexplained)
            .count()
    }
}

/// Instructions given to the model.
///
/// The constraints here exist because the failure mode of this feature is a
/// confident wrong answer sending someone to reinstall their drivers over a
/// full disk.
const SYSTEM_PROMPT: &str = "\
You are the analysis layer of a computer diagnostic tool. You are given findings \
that deterministic checks have already produced. You cannot inspect the machine \
and you cannot run anything; the findings and their evidence are all you have.

Your job is to explain what these findings mean for the person using this \
computer, and to correlate them: findings that share a cause should be described \
as one problem, not several.

Rules you must follow:
- Write for someone who is not an expert. No jargon without explaining it.
- Say what you actually know. If the evidence does not identify a cause, say the \
cause is not identifiable from this evidence and say what would identify it.
- Never invent error codes, file paths, version numbers, or log lines. Use only \
what appears in the evidence given to you.
- Prefer the least invasive fix that could work. Never suggest deleting data, \
formatting anything, or reinstalling an operating system.
- A correlation you are unsure about is worth stating as a possibility, clearly \
labelled as one. A guess presented as a fact is not.

Reply with a single JSON object and nothing else, in this shape:
{
  \"correlation\": \"what these findings mean taken together, or null if they are unrelated\",
  \"findings\": [
    {
      \"finding_id\": \"the id exactly as given\",
      \"subject\": \"the subject exactly as given, or null\",
      \"explanation\": \"plain-language explanation\",
      \"fixes\": [
        {\"description\": \"what to do\", \"invasiveness\": \"inspect|low|medium|high\"}
      ]
    }
  ]
}";

/// The model's reply, before it is trusted.
#[derive(Debug, Deserialize)]
struct ModelAnalysis {
    #[serde(default)]
    correlation: Option<String>,
    #[serde(default)]
    findings: Vec<ModelFinding>,
}

#[derive(Debug, Deserialize)]
struct ModelFinding {
    #[serde(default)]
    finding_id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    explanation: String,
    #[serde(default)]
    fixes: Vec<ModelFix>,
}

#[derive(Debug, Deserialize)]
struct ModelFix {
    #[serde(default)]
    description: String,
    #[serde(default)]
    invasiveness: Option<Invasiveness>,
}

/// Pull the first complete JSON object out of a model's reply.
///
/// Models wrap JSON in prose and in code fences no matter how firmly they are
/// asked not to, and a local model at 4-bit quantisation does it more often
/// than a large one. Scanning for the object is the difference between this
/// feature working on modest hardware and not.
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, byte) in text.as_bytes()[start..].iter().copied().enumerate() {
        let index = start + offset;
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(start..=index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Render findings for the model in a compact, unambiguous form.
fn describe_findings(findings: &[&Finding], needs_explanation: &BTreeSet<String>) -> String {
    let mut text = String::new();
    for finding in findings {
        let key = finding.occurrence_key();
        text.push_str(&format!(
            "\n---\nfinding_id: {}\nsubject: {}\nseverity: {}\ncategory: {}\ntitle: {}\ndetail: {}\n",
            finding.id,
            finding.subject.as_deref().unwrap_or("none"),
            finding.severity,
            finding.category,
            finding.title,
            finding.detail,
        ));
        for item in &finding.evidence {
            text.push_str(&format!("evidence.{}: {}\n", item.label, item.value));
        }
        text.push_str(&format!(
            "needs_explanation: {}\n",
            if needs_explanation.contains(&key) {
                "yes"
            } else {
                "no -- already answered"
            }
        ));
    }
    text
}

/// Analyses scan reports, runbooks first.
pub struct Analyst {
    library: RunbookLibrary,
    platform: String,
}

impl Analyst {
    pub fn new(library: RunbookLibrary, platform: impl Into<String>) -> Self {
        Self {
            library,
            platform: platform.into(),
        }
    }

    pub fn library(&self) -> &RunbookLibrary {
        &self.library
    }

    /// Explain a scan.
    ///
    /// `routing` decides whether a model is consulted at all. When none is
    /// available this still returns everything the runbook library knows.
    pub async fn analyse(&self, report: &ScanReport, routing: &Routing) -> Result<Analysis> {
        let findings = report.findings();
        if findings.is_empty() {
            return Ok(Analysis {
                correlation: None,
                items: Vec::new(),
                model: None,
                answered_by_runbook: 0,
            });
        }

        let mut items = Vec::new();
        let mut unanswered = BTreeSet::new();

        for finding in &findings {
            match self.library.lookup(finding) {
                Some(entry) => items.push(AnalysedFinding {
                    finding_id: finding.id.clone(),
                    subject: finding.subject.clone(),
                    title: finding.title.clone(),
                    severity: finding.severity,
                    source: AnalysisSource::Runbook {
                        entry_id: entry.id.clone(),
                    },
                    explanation: entry.explanation.trim().to_string(),
                    fixes: entry
                        .fixes_for(&self.platform)
                        .into_iter()
                        .cloned()
                        .collect(),
                }),
                None => {
                    unanswered.insert(finding.occurrence_key());
                    items.push(AnalysedFinding {
                        finding_id: finding.id.clone(),
                        subject: finding.subject.clone(),
                        title: finding.title.clone(),
                        severity: finding.severity,
                        source: AnalysisSource::Unexplained,
                        explanation: String::new(),
                        fixes: Vec::new(),
                    });
                }
            }
        }

        let answered_by_runbook = items.len() - unanswered.len();

        let Some(client) = routing.client.as_ref() else {
            tracing::debug!("no model available; runbook answers only");
            for item in &mut items {
                if item.source == AnalysisSource::Unexplained {
                    item.explanation =
                        "No runbook entry covers this, and no model was available to reason \
                         about it. The finding above is what the deterministic checks \
                         established on their own."
                            .to_string();
                }
            }
            return Ok(Analysis {
                correlation: None,
                items,
                model: None,
                answered_by_runbook,
            });
        };

        // Even when every finding has a runbook answer, the model is still
        // worth asking: correlating findings across subsystems is the thing it
        // adds that a lookup table cannot.
        let prompt = describe_findings(&findings, &unanswered);
        match self.consult(client.as_ref(), &prompt).await {
            Ok((model, analysis)) => {
                apply_model_analysis(&mut items, &analysis, &model);
                Ok(Analysis {
                    correlation: analysis.correlation.filter(|text| !text.trim().is_empty()),
                    items,
                    model: Some(model),
                    answered_by_runbook,
                })
            }
            Err(error) => {
                // A model that is unreachable or unhelpful must not cost the
                // user the runbook answers that were already found.
                tracing::warn!(%error, "model analysis failed; keeping runbook answers");
                for item in &mut items {
                    if item.source == AnalysisSource::Unexplained {
                        item.explanation = format!(
                            "No runbook entry covers this, and the model could not be \
                             consulted ({error})."
                        );
                    }
                }
                Ok(Analysis {
                    correlation: None,
                    items,
                    model: None,
                    answered_by_runbook,
                })
            }
        }
    }

    async fn consult(
        &self,
        client: &dyn ModelClient,
        prompt: &str,
    ) -> Result<(String, ModelAnalysis)> {
        let completion = client.complete(SYSTEM_PROMPT, prompt).await?;
        let json = extract_json_object(&completion.text)
            .ok_or_else(|| anyhow::anyhow!("the model did not return JSON"))?;
        let analysis: ModelAnalysis = serde_json::from_str(json)
            .map_err(|error| anyhow::anyhow!("the model returned malformed JSON: {error}"))?;
        Ok((completion.model, analysis))
    }
}

/// Fold the model's answers into the findings they belong to.
///
/// Answers are matched back by identifier. Anything the model invented that
/// does not correspond to a real finding is dropped rather than shown -- a
/// hallucinated problem presented next to real ones would poison the whole
/// report.
fn apply_model_analysis(items: &mut [AnalysedFinding], analysis: &ModelAnalysis, model: &str) {
    for answer in &analysis.findings {
        let subject = answer
            .subject
            .as_deref()
            .filter(|s| !s.eq_ignore_ascii_case("none"));
        let Some(item) = items.iter_mut().find(|item| {
            item.finding_id == answer.finding_id
                && (subject.is_none() || item.subject.as_deref() == subject)
        }) else {
            tracing::debug!(
                finding_id = answer.finding_id,
                "model answered about a finding that was not in the scan; dropping it"
            );
            continue;
        };

        // A runbook answer is deterministic and reviewed. It is not replaced by
        // a model's opinion of the same problem.
        if matches!(item.source, AnalysisSource::Runbook { .. }) {
            continue;
        }
        if answer.explanation.trim().is_empty() {
            continue;
        }

        item.source = AnalysisSource::Model {
            model: model.to_string(),
        };
        item.explanation = answer.explanation.trim().to_string();
        item.fixes = answer
            .fixes
            .iter()
            .filter(|fix| !fix.description.trim().is_empty())
            .map(|fix| CandidateFix {
                description: fix.description.trim().to_string(),
                invasiveness: fix.invasiveness.unwrap_or(Invasiveness::Low),
                // The model is not given the ability to propose a command to
                // run. Its suggestions are prose that a person reads and
                // decides on.
                command: None,
                platforms: Vec::new(),
            })
            .collect();
        item.fixes.sort_by_key(|fix| fix.invasiveness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, subject: Option<&str>, source: AnalysisSource) -> AnalysedFinding {
        AnalysedFinding {
            finding_id: id.to_string(),
            subject: subject.map(str::to_string),
            title: "t".to_string(),
            severity: Severity::High,
            source,
            explanation: String::new(),
            fixes: Vec::new(),
        }
    }

    fn model_answer(id: &str, explanation: &str) -> ModelFinding {
        ModelFinding {
            finding_id: id.to_string(),
            subject: None,
            explanation: explanation.to_string(),
            fixes: vec![ModelFix {
                description: "do the thing".to_string(),
                invasiveness: Some(Invasiveness::Medium),
            }],
        }
    }

    #[test]
    fn json_is_recovered_from_a_reply_wrapped_in_prose_and_fences() {
        // Small local models do this constantly, however firmly they are asked
        // not to.
        let reply =
            "Sure! Here is the analysis:\n```json\n{\"correlation\": \"x\"}\n```\nHope that helps.";
        assert_eq!(extract_json_object(reply), Some("{\"correlation\": \"x\"}"));
    }

    #[test]
    fn nested_objects_are_matched_to_the_correct_closing_brace() {
        let reply = "{\"a\": {\"b\": 1}, \"c\": 2} trailing junk";
        assert_eq!(
            extract_json_object(reply),
            Some("{\"a\": {\"b\": 1}, \"c\": 2}")
        );
    }

    #[test]
    fn braces_inside_strings_do_not_confuse_the_scanner() {
        let reply = r#"{"text": "a } brace and a \" quote"}"#;
        let extracted = extract_json_object(reply).expect("should extract");
        assert_eq!(extracted, reply);
        assert!(serde_json::from_str::<serde_json::Value>(extracted).is_ok());
    }

    #[test]
    fn a_reply_with_no_json_yields_nothing_rather_than_garbage() {
        assert_eq!(extract_json_object("I could not analyse this."), None);
        assert_eq!(extract_json_object("{unterminated"), None);
    }

    #[test]
    fn a_model_answer_fills_in_an_unexplained_finding() {
        let mut items = vec![item(
            "logs.repeated-error",
            Some("SomeService"),
            AnalysisSource::Unexplained,
        )];
        let analysis = ModelAnalysis {
            correlation: None,
            findings: vec![model_answer("logs.repeated-error", "here is what it means")],
        };

        apply_model_analysis(&mut items, &analysis, "test-model");
        assert_eq!(
            items[0].source,
            AnalysisSource::Model {
                model: "test-model".to_string()
            }
        );
        assert_eq!(items[0].explanation, "here is what it means");
        assert_eq!(items[0].fixes.len(), 1);
    }

    #[test]
    fn a_runbook_answer_is_never_overwritten_by_the_model() {
        // Runbook entries are deterministic and have been reviewed. A model's
        // opinion does not get to replace one.
        let mut items = vec![item(
            "memory.high-pressure",
            None,
            AnalysisSource::Runbook {
                entry_id: "memory.high-pressure".to_string(),
            },
        )];
        items[0].explanation = "the reviewed answer".to_string();

        let analysis = ModelAnalysis {
            correlation: None,
            findings: vec![model_answer("memory.high-pressure", "a different opinion")],
        };
        apply_model_analysis(&mut items, &analysis, "test-model");

        assert_eq!(items[0].explanation, "the reviewed answer");
        assert!(matches!(items[0].source, AnalysisSource::Runbook { .. }));
    }

    #[test]
    fn an_invented_finding_is_dropped_rather_than_reported() {
        // A hallucinated problem shown next to real ones would poison the
        // credibility of the whole report.
        let mut items = vec![item("real.finding", None, AnalysisSource::Unexplained)];
        let analysis = ModelAnalysis {
            correlation: None,
            findings: vec![model_answer("invented.finding", "this never happened")],
        };

        apply_model_analysis(&mut items, &analysis, "test-model");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, AnalysisSource::Unexplained);
    }

    #[test]
    fn an_empty_model_explanation_leaves_the_finding_unexplained() {
        let mut items = vec![item("some.finding", None, AnalysisSource::Unexplained)];
        let analysis = ModelAnalysis {
            correlation: None,
            findings: vec![model_answer("some.finding", "   ")],
        };

        apply_model_analysis(&mut items, &analysis, "test-model");
        assert_eq!(items[0].source, AnalysisSource::Unexplained);
    }

    #[test]
    fn model_answers_are_matched_by_subject_when_one_is_given() {
        let mut items = vec![
            item(
                "storage.volume-low-on-space",
                Some("C:"),
                AnalysisSource::Unexplained,
            ),
            item(
                "storage.volume-low-on-space",
                Some("D:"),
                AnalysisSource::Unexplained,
            ),
        ];
        let mut answer = model_answer("storage.volume-low-on-space", "about D only");
        answer.subject = Some("D:".to_string());

        apply_model_analysis(
            &mut items,
            &ModelAnalysis {
                correlation: None,
                findings: vec![answer],
            },
            "test-model",
        );

        assert_eq!(
            items[0].source,
            AnalysisSource::Unexplained,
            "C: should be untouched"
        );
        assert_eq!(items[1].explanation, "about D only");
    }

    #[test]
    fn model_fixes_are_ordered_least_invasive_first() {
        let mut items = vec![item("some.finding", None, AnalysisSource::Unexplained)];
        let analysis = ModelAnalysis {
            correlation: None,
            findings: vec![ModelFinding {
                finding_id: "some.finding".to_string(),
                subject: None,
                explanation: "explained".to_string(),
                fixes: vec![
                    ModelFix {
                        description: "reinstall everything".to_string(),
                        invasiveness: Some(Invasiveness::High),
                    },
                    ModelFix {
                        description: "look at the log".to_string(),
                        invasiveness: Some(Invasiveness::Inspect),
                    },
                ],
            }],
        };

        apply_model_analysis(&mut items, &analysis, "test-model");
        assert_eq!(items[0].fixes[0].invasiveness, Invasiveness::Inspect);
        assert_eq!(items[0].fixes[1].invasiveness, Invasiveness::High);
    }

    #[test]
    fn the_model_is_never_given_a_command_to_run() {
        // Runbook commands are written by a person and reviewed. A model's
        // suggestions stay prose.
        let mut items = vec![item("some.finding", None, AnalysisSource::Unexplained)];
        apply_model_analysis(
            &mut items,
            &ModelAnalysis {
                correlation: None,
                findings: vec![model_answer("some.finding", "explained")],
            },
            "test-model",
        );
        assert!(items[0].fixes.iter().all(|fix| fix.command.is_none()));
    }
}
