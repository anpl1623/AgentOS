//! A delegation tree: who asked whom, one node per run, depth first.

use agentos_runtime::DelegationNode;

use super::{Style, WIDTH, summary_line, truncate, visible_width};

/// Render a delegation tree, depth first, one node per run.
///
/// Each node is the agent and how its run stands, then what it was asked and
/// the task to `show` for that run's own trace. The tree says who asked whom;
/// what any one of them did is the flat trace's job.
#[must_use]
pub fn delegation_tree(root: &DelegationNode, style: &Style) -> String {
    let mut out = String::new();
    tree_node(root, "", "", style, &mut out);
    out
}

/// One node and everything beneath it.
///
/// `lead` prefixes the node's own line and `rest` every line after it, so a
/// branch that has further siblings below keeps its vertical rule running past
/// this node's children.
fn tree_node(node: &DelegationNode, lead: &str, rest: &str, style: &Style, out: &mut String) {
    let run = &node.run;
    out.push_str(&format!(
        "{}{}\n",
        style.dim(lead),
        summary_line(
            &node.agent_name,
            &style.state(run.state),
            run.steps_taken,
            run.tainted,
            style
        )
    ));

    let body = if node.children.is_empty() {
        format!("{rest}   ")
    } else {
        format!("{rest}│  ")
    };
    // Measured in columns like the objective it makes room for, and one short
    // of the edge, because a cut objective ends in an ellipsis that takes a
    // column of its own.
    let room = WIDTH.saturating_sub(visible_width(&body) + 1);
    out.push_str(&format!(
        "{}{}\n",
        style.dim(&body),
        truncate(&one_line(&node.objective), room)
    ));
    out.push_str(&format!(
        "{}{}\n",
        style.dim(&body),
        style.dim(&format!("task {}", run.task_id))
    ));

    for (index, child) in node.children.iter().enumerate() {
        let (branch, under) = if index + 1 == node.children.len() {
            ("└─ ", "   ")
        } else {
            ("├─ ", "│  ")
        };
        tree_node(
            child,
            &format!("{rest}{branch}"),
            &format!("{rest}{under}"),
            style,
            out,
        );
    }
}

/// Text collapsed onto one line, with control characters removed.
///
/// A child's objective was written by its parent's model. A newline in it
/// would break the tree's columns, and an escape sequence would be the model
/// writing to the operator's terminal.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use agentos_core::task::{TaskRun, TaskState};

    use super::*;
    use crate::render::test_support::{child_run, plain};

    fn node(
        agent: &str,
        objective: &str,
        run: TaskRun,
        children: Vec<DelegationNode>,
    ) -> DelegationNode {
        DelegationNode {
            run,
            agent_name: agent.into(),
            objective: objective.into(),
            delegation: None,
            children,
        }
    }

    #[test]
    fn the_tree_nests_each_child_under_the_run_that_asked() {
        let fetcher = node(
            "fetcher",
            "Fetch",
            child_run(TaskState::Completed, 1, true),
            vec![],
        );
        let fetcher_task = fetcher.run.task_id;
        let root = node(
            "supervisor",
            "Summarise the week",
            child_run(TaskState::Completed, 4, true),
            vec![
                node(
                    "researcher",
                    "Find the orders",
                    child_run(TaskState::Completed, 3, true),
                    vec![fetcher],
                ),
                node(
                    "writer",
                    "Draft it",
                    child_run(TaskState::Failed, 1, false),
                    vec![],
                ),
            ],
        );
        let text = delegation_tree(&root, &plain());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                "supervisor · completed · 4 steps · tainted".to_owned(),
                "│  Summarise the week".to_owned(),
                format!("│  task {}", root.run.task_id),
                "├─ researcher · completed · 3 steps · tainted".to_owned(),
                "│  │  Find the orders".to_owned(),
                format!("│  │  task {}", root.children[0].run.task_id),
                "│  └─ fetcher · completed · 1 step · tainted".to_owned(),
                "│        Fetch".to_owned(),
                format!("│        task {fetcher_task}"),
                "└─ writer · failed · 1 step".to_owned(),
                "      Draft it".to_owned(),
                format!("      task {}", root.children[1].run.task_id),
            ],
            "{text}"
        );
    }

    #[test]
    fn a_long_objective_is_cut_inside_the_width() {
        // A Hangul syllable takes two columns, so an objective cut by
        // characters ran to nearly twice the width. The ellipsis takes a
        // column too, which a long English objective used to spend past the
        // edge.
        use unicode_width::UnicodeWidthStr;

        for objective in [
            "이슈 제목을 읽고 요약한 다음 담당자에게 알려 주세요 ".repeat(6),
            "日本語のタイトルを読んで要約してください🙂".repeat(6),
            "read the issue and summarise it for the owner ".repeat(6),
        ] {
            let root = node(
                "supervisor",
                &objective,
                child_run(TaskState::Completed, 2, false),
                vec![node(
                    "researcher",
                    &objective,
                    child_run(TaskState::Completed, 1, false),
                    vec![],
                )],
            );
            let text = delegation_tree(&root, &plain());
            assert!(text.contains('…'), "{text}");
            for line in text.lines() {
                assert!(line.width() <= WIDTH, "{} columns: {line}", line.width());
            }
        }
    }

    #[test]
    fn a_model_written_objective_cannot_write_to_the_terminal() {
        let root = node(
            "supervisor",
            "line one\nline two \u{1b}[2Jcleared",
            child_run(TaskState::Completed, 1, false),
            vec![],
        );
        let text = delegation_tree(&root, &Style { enabled: true });
        assert!(!text.contains("\u{1b}[2J"), "{text:?}");
        assert!(text.contains("line one line two [2Jcleared"), "{text:?}");
        assert_eq!(text.lines().count(), 3, "{text:?}");
    }
}
