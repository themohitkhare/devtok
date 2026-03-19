use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::db::Db;
use crate::models::{pricing, KnowledgeEntry, Ticket};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Markdown,
    Json,
}

#[derive(Debug, Serialize)]
struct ExportTicket {
    id: String,
    title: String,
    status: String,
    assignee: Option<String>,
    completion_time: Option<String>,
}

#[derive(Debug, Serialize)]
struct MergeEntry {
    timestamp: String,
    commit: String,
    subject: String,
    branch: Option<String>,
}

#[derive(Debug, Serialize)]
struct CostEstimate {
    sonnet_usd: f64,
    opus_usd: f64,
}

#[derive(Debug, Serialize)]
struct TokenUsageSummary {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    estimated_cost: CostEstimate,
}

#[derive(Debug, Serialize)]
struct AdrEntry {
    domain: String,
    key: String,
    value: String,
    version: i32,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct ExportSummary {
    generated_at: String,
    tickets: Vec<ExportTicket>,
    merged_branches: Vec<MergeEntry>,
    knowledge_by_domain: BTreeMap<String, Vec<KnowledgeEntry>>,
    token_usage: TokenUsageSummary,
    architecture_decisions: Vec<AdrEntry>,
}

pub fn execute(format: ExportFormat, out: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    let summary = collect_summary(&db)?;
    let content = match format {
        ExportFormat::Markdown => render_markdown(&summary),
        ExportFormat::Json => serde_json::to_string_pretty(&summary)?,
    };

    if let Some(out_file) = out {
        let path = PathBuf::from(out_file);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&path, content)?;
        eprintln!("[export] wrote {}", path.display());
    } else {
        println!("{}", content);
    }

    Ok(())
}

fn collect_summary(db: &Db) -> Result<ExportSummary> {
    let tickets = db.list_tickets(None)?;
    let export_tickets = tickets_to_summary(&tickets);
    let merged_branches = merged_branches_from_git().unwrap_or_default();
    let knowledge = db.list_all_knowledge()?;
    let knowledge_by_domain = group_knowledge_by_domain(&knowledge);
    let architecture_decisions = extract_adrs(&knowledge);
    let (input_tokens, output_tokens) = db.total_token_details()?;
    let total_tokens = input_tokens + output_tokens;

    let token_usage = TokenUsageSummary {
        input_tokens,
        output_tokens,
        total_tokens,
        estimated_cost: CostEstimate {
            sonnet_usd: pricing::estimate_cost(
                input_tokens,
                output_tokens,
                pricing::SONNET_INPUT_PER_M,
                pricing::SONNET_OUTPUT_PER_M,
            ),
            opus_usd: pricing::estimate_cost(
                input_tokens,
                output_tokens,
                pricing::OPUS_INPUT_PER_M,
                pricing::OPUS_OUTPUT_PER_M,
            ),
        },
    };

    Ok(ExportSummary {
        generated_at: chrono::Utc::now().to_rfc3339(),
        tickets: export_tickets,
        merged_branches,
        knowledge_by_domain,
        token_usage,
        architecture_decisions,
    })
}

fn tickets_to_summary(tickets: &[Ticket]) -> Vec<ExportTicket> {
    tickets
        .iter()
        .map(|t| ExportTicket {
            id: t.id.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            assignee: t.assignee.clone(),
            completion_time: (t.status == "completed").then(|| t.updated_at.clone()),
        })
        .collect()
}

fn group_knowledge_by_domain(entries: &[KnowledgeEntry]) -> BTreeMap<String, Vec<KnowledgeEntry>> {
    let mut grouped = BTreeMap::new();
    for entry in entries {
        grouped
            .entry(entry.domain.clone())
            .or_insert_with(Vec::new)
            .push(entry.clone());
    }
    grouped
}

fn extract_adrs(entries: &[KnowledgeEntry]) -> Vec<AdrEntry> {
    entries
        .iter()
        .filter(|e| {
            let normalized_key = e.key.to_ascii_lowercase();
            normalized_key.starts_with("adr") || normalized_key.starts_with("decision")
        })
        .map(|e| AdrEntry {
            domain: e.domain.clone(),
            key: e.key.clone(),
            value: e.value.clone(),
            version: e.version,
            updated_at: e.updated_at.clone(),
        })
        .collect()
}

fn merged_branches_from_git() -> Result<Vec<MergeEntry>> {
    let output = Command::new("git")
        .args([
            "log",
            "--merges",
            "--date=iso-strict",
            "--pretty=format:%H|%ad|%s",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git log --merges failed");
    }

    let lines = String::from_utf8_lossy(&output.stdout);
    let mut merged = Vec::new();
    for line in lines.lines() {
        let mut parts = line.splitn(3, '|');
        let commit = parts.next().unwrap_or_default().to_string();
        let timestamp = parts.next().unwrap_or_default().to_string();
        let subject = parts.next().unwrap_or_default().to_string();
        let branch = extract_branch_from_merge_subject(&subject);
        if !commit.is_empty() {
            merged.push(MergeEntry {
                timestamp,
                commit: commit.chars().take(12).collect(),
                subject,
                branch,
            });
        }
    }
    Ok(merged)
}

fn extract_branch_from_merge_subject(subject: &str) -> Option<String> {
    let prefix = "Merge branch '";
    let start = subject.find(prefix)? + prefix.len();
    let rest = &subject[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn render_markdown(summary: &ExportSummary) -> String {
    let mut out = String::new();
    out.push_str("# ACS Project Summary Export\n\n");
    out.push_str(&format!("_Generated: {}_\n\n", summary.generated_at));

    out.push_str("## Tickets\n\n");
    out.push_str("| ID | Title | Status | Assignee | Completion Time |\n");
    out.push_str("|----|-------|--------|----------|-----------------|\n");
    for t in &summary.tickets {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            t.id,
            t.title,
            t.status,
            t.assignee.as_deref().unwrap_or("-"),
            t.completion_time.as_deref().unwrap_or("-")
        ));
    }
    out.push('\n');

    out.push_str("## Git Log (Merged Branches)\n\n");
    if summary.merged_branches.is_empty() {
        out.push_str("_No merge commits found._\n\n");
    } else {
        for m in &summary.merged_branches {
            match &m.branch {
                Some(branch) => out.push_str(&format!(
                    "- `{}` `{}` merged `{}` — {}\n",
                    m.timestamp, m.commit, branch, m.subject
                )),
                None => out.push_str(&format!(
                    "- `{}` `{}` — {}\n",
                    m.timestamp, m.commit, m.subject
                )),
            }
        }
        out.push('\n');
    }

    out.push_str("## Knowledge Base (Grouped by Domain)\n\n");
    if summary.knowledge_by_domain.is_empty() {
        out.push_str("_No knowledge base entries found._\n\n");
    } else {
        for (domain, entries) in &summary.knowledge_by_domain {
            out.push_str(&format!("### {}\n\n", domain));
            for entry in entries {
                out.push_str(&format!(
                    "- `{}` (v{}, updated {}): {}\n",
                    entry.key, entry.version, entry.updated_at, entry.value
                ));
            }
            out.push('\n');
        }
    }

    out.push_str("## Token Usage and Estimated Cost\n\n");
    out.push_str(&format!(
        "- Input tokens: `{}`\n- Output tokens: `{}`\n- Total tokens: `{}`\n- Estimated Sonnet cost: `${:.4}`\n- Estimated Opus cost: `${:.4}`\n\n",
        summary.token_usage.input_tokens,
        summary.token_usage.output_tokens,
        summary.token_usage.total_tokens,
        summary.token_usage.estimated_cost.sonnet_usd,
        summary.token_usage.estimated_cost.opus_usd
    ));

    out.push_str("## Architecture Decisions (ADRs)\n\n");
    if summary.architecture_decisions.is_empty() {
        out.push_str("_No ADR entries found in KB._\n");
    } else {
        for adr in &summary.architecture_decisions {
            out.push_str(&format!(
                "### `{}/{}`\n\n{}\n\n",
                adr.domain, adr.key, adr.value
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_extraction_from_merge_subject() {
        let branch = extract_branch_from_merge_subject("Merge branch 'acs/t-034-abc' into main");
        assert_eq!(branch.as_deref(), Some("acs/t-034-abc"));
    }

    #[test]
    fn branch_extraction_handles_nonstandard_subject() {
        let branch = extract_branch_from_merge_subject("Merge pull request #12 from team/feature");
        assert!(branch.is_none());
    }

    #[test]
    fn markdown_contains_required_sections() {
        let mut kb = BTreeMap::new();
        kb.insert(
            "core".to_string(),
            vec![KnowledgeEntry {
                domain: "core".to_string(),
                key: "stack".to_string(),
                value: "Rust".to_string(),
                version: 1,
                updated_at: "2026-03-20T00:00:00Z".to_string(),
            }],
        );
        let summary = ExportSummary {
            generated_at: "2026-03-20T00:00:00Z".to_string(),
            tickets: vec![ExportTicket {
                id: "t-001".to_string(),
                title: "Example".to_string(),
                status: "completed".to_string(),
                assignee: Some("w-1".to_string()),
                completion_time: Some("2026-03-20T00:00:00Z".to_string()),
            }],
            merged_branches: vec![MergeEntry {
                timestamp: "2026-03-20T00:00:00Z".to_string(),
                commit: "abcdef123456".to_string(),
                subject: "Merge branch 'acs/t-001-foo' into main".to_string(),
                branch: Some("acs/t-001-foo".to_string()),
            }],
            knowledge_by_domain: kb,
            token_usage: TokenUsageSummary {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                estimated_cost: CostEstimate {
                    sonnet_usd: 0.01,
                    opus_usd: 0.02,
                },
            },
            architecture_decisions: vec![AdrEntry {
                domain: "architecture".to_string(),
                key: "adr-001".to_string(),
                value: "Use SQLite".to_string(),
                version: 1,
                updated_at: "2026-03-20T00:00:00Z".to_string(),
            }],
        };

        let markdown = render_markdown(&summary);
        assert!(markdown.contains("## Tickets"));
        assert!(markdown.contains("## Git Log (Merged Branches)"));
        assert!(markdown.contains("## Knowledge Base (Grouped by Domain)"));
        assert!(markdown.contains("## Token Usage and Estimated Cost"));
        assert!(markdown.contains("## Architecture Decisions (ADRs)"));
    }

    #[test]
    fn adr_extraction_is_case_insensitive() {
        let entries = vec![
            KnowledgeEntry {
                domain: "architecture".to_string(),
                key: "ADR-001".to_string(),
                value: "Use SQLite".to_string(),
                version: 1,
                updated_at: "2026-03-20T00:00:00Z".to_string(),
            },
            KnowledgeEntry {
                domain: "architecture".to_string(),
                key: "Decision-Auth-Flow".to_string(),
                value: "Use token auth".to_string(),
                version: 1,
                updated_at: "2026-03-20T00:00:00Z".to_string(),
            },
            KnowledgeEntry {
                domain: "core".to_string(),
                key: "stack".to_string(),
                value: "Rust".to_string(),
                version: 1,
                updated_at: "2026-03-20T00:00:00Z".to_string(),
            },
        ];

        let adrs = extract_adrs(&entries);
        assert_eq!(adrs.len(), 2);
        assert!(adrs.iter().any(|entry| entry.key == "ADR-001"));
        assert!(adrs
            .iter()
            .any(|entry| entry.key == "Decision-Auth-Flow"));
    }
}
