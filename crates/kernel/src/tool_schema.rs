//! Read-only ToolCatalog schema for Caravan.
//!
//! Defines the static catalog of available manual tools and a plain-text
//! renderer for inclusion in model prompts. This is NOT JSON Schema / OpenAI
//! function-calling / MCP — plain Rust structs and a plain-text renderer only.

use crate::tools::ToolRisk;

/// Describes a single input parameter accepted by a tool.
pub struct ToolInputSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// Describes a single tool: its name, purpose, risk level, and accepted inputs.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub risk: ToolRisk,
    pub inputs: Vec<ToolInputSpec>,
}

/// Static catalog of tools available in Caravan.
pub struct ToolCatalog {
    specs: Vec<ToolSpec>,
}

impl ToolCatalog {
    /// Returns the read-only catalog containing exactly the two supported tools.
    pub fn readonly() -> Self {
        ToolCatalog {
            specs: vec![
                ToolSpec {
                    name: "list_files",
                    description: "List files in a workspace-relative directory. Read-only. Non-recursive.",
                    risk: ToolRisk::ReadOnly,
                    inputs: vec![ToolInputSpec {
                        name: "path",
                        description: "Workspace-relative directory to list. Defaults to \".\".",
                        required: false,
                    }],
                },
                ToolSpec {
                    name: "read_file",
                    description: "Read a UTF-8 text file under the workspace. Read-only. Size-limited.",
                    risk: ToolRisk::ReadOnly,
                    inputs: vec![ToolInputSpec {
                        name: "path",
                        description: "Workspace-relative path to the file to read.",
                        required: true,
                    }],
                },
            ],
        }
    }

    /// Returns all tool specs in the catalog.
    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// Renders a complete plain-text prompt section describing all available tools.
    ///
    /// The returned string begins with the `Available Tools:` header line and
    /// contains guidance text followed by one block per tool. It never embeds
    /// file content or tool output.
    pub fn render_prompt_section(&self) -> String {
        let mut out = String::new();

        out.push_str("Available Tools:\n");
        out.push_str(
            "These tools are read-only and are available manually through Caravan slash commands.\n",
        );
        out.push_str(
            "The model cannot call these tools automatically in this POC — \
             tool invocation always requires an explicit user command.\n",
        );
        out.push_str(
            "Tool output is not included in the prompt unless the user runs \
             `/context attach-last-tool`.\n",
        );

        out.push_str(
            "\nModel Tool Request Blocks:\n\
             You may propose a tool request by emitting a CARAVAN_TOOL_REQUEST block in your \
             response. Caravan will detect the block and record a ModelToolRequest. The request \
             is not executed automatically — the user must run the matching /tool command \
             manually, and tool output enters the prompt only if the user runs \
             `/context attach-last-tool`. \
             To inspect the recorded request run `/request status`; \
             to discard it run `/request clear`.\n\
             \n\
             Example (read_file):\n\
             CARAVAN_TOOL_REQUEST\n\
             tool=read_file\n\
             path=README.md\n\
             END_CARAVAN_TOOL_REQUEST\n\
             \n\
             Example (list_files):\n\
             CARAVAN_TOOL_REQUEST\n\
             tool=list_files\n\
             path=.\n\
             END_CARAVAN_TOOL_REQUEST\n",
        );

        for spec in &self.specs {
            out.push('\n');

            // Determine the slash command for this tool.
            let command = match spec.name {
                "list_files" => "/tool list [path]",
                "read_file" => "/tool read <path>",
                other => other,
            };

            out.push_str(&format!("Tool: {}\n", spec.name));
            out.push_str(&format!("  Command:     {}\n", command));
            out.push_str(&format!("  Risk:        {}\n", spec.risk.as_str()));
            out.push_str(&format!("  Description: {}\n", spec.description));

            if !spec.inputs.is_empty() {
                out.push_str("  Inputs:\n");
                for input in &spec.inputs {
                    let required_label = if input.required {
                        "required"
                    } else {
                        "optional"
                    };
                    out.push_str(&format!(
                        "    - {} ({}): {}\n",
                        input.name, required_label, input.description
                    ));
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRisk;

    #[test]
    fn readonly_returns_exactly_two_specs() {
        let catalog = ToolCatalog::readonly();
        assert_eq!(catalog.specs().len(), 2);
    }

    #[test]
    fn readonly_spec_names_are_list_files_and_read_file() {
        let catalog = ToolCatalog::readonly();
        let names: Vec<&str> = catalog.specs().iter().map(|s| s.name).collect();
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"read_file"));
    }

    #[test]
    fn all_specs_have_read_only_risk() {
        let catalog = ToolCatalog::readonly();
        for spec in catalog.specs() {
            assert_eq!(spec.risk, ToolRisk::ReadOnly);
        }
    }

    #[test]
    fn render_prompt_section_contains_required_strings() {
        let catalog = ToolCatalog::readonly();
        let section = catalog.render_prompt_section();

        assert!(
            section.contains("Available Tools:"),
            "missing 'Available Tools:' header"
        );
        assert!(
            section.contains("/tool list [path]"),
            "missing list_files command"
        );
        assert!(
            section.contains("/tool read <path>"),
            "missing read_file command"
        );
        assert!(
            section.contains("list_files"),
            "missing list_files tool name"
        );
        assert!(section.contains("read_file"), "missing read_file tool name");
        assert!(
            section.contains("cannot call these tools automatically"),
            "missing phrase about model not being able to call tools automatically"
        );
        assert!(
            section.contains("/context attach-last-tool"),
            "missing /context attach-last-tool reference"
        );

        // CARAVAN_TOOL_REQUEST block guidance assertions.
        assert!(
            section.contains("CARAVAN_TOOL_REQUEST"),
            "missing CARAVAN_TOOL_REQUEST marker"
        );
        assert!(
            section.contains("tool=read_file"),
            "missing read_file example block"
        );
        assert!(
            section.contains("tool=list_files"),
            "missing list_files example block"
        );
        assert!(
            section.contains("not executed automatically"),
            "missing 'not executed automatically' phrase"
        );
        assert!(
            section.contains("ModelToolRequest"),
            "missing ModelToolRequest reference"
        );
        assert!(
            section.contains("/request status"),
            "missing /request status reference"
        );
        assert!(
            section.contains("/request clear"),
            "missing /request clear reference"
        );

        // Forbidden phrases must NOT appear (built at runtime to avoid grep false-positives).
        let forbidden_auto_exec = ["Caravan will execute", " this automatically"].concat();
        assert!(
            !section.contains(forbidden_auto_exec.as_str()),
            "forbidden auto-exec phrase found in prompt section"
        );
        let forbidden_model_call = ["The model can", " call tools"].concat();
        assert!(
            !section.contains(forbidden_model_call.as_str()),
            "forbidden model-call phrase found in prompt section"
        );
    }

    #[test]
    fn render_prompt_section_contains_read_only_risk() {
        let catalog = ToolCatalog::readonly();
        let section = catalog.render_prompt_section();
        assert!(
            section.contains("read_only"),
            "missing read_only risk string"
        );
    }

    #[test]
    fn list_files_path_input_is_optional() {
        let catalog = ToolCatalog::readonly();
        let list_files_spec = catalog
            .specs()
            .iter()
            .find(|s| s.name == "list_files")
            .expect("list_files spec not found");
        let path_input = list_files_spec
            .inputs
            .iter()
            .find(|i| i.name == "path")
            .expect("path input not found");
        assert!(!path_input.required, "list_files path should be optional");
    }

    #[test]
    fn read_file_path_input_is_required() {
        let catalog = ToolCatalog::readonly();
        let read_file_spec = catalog
            .specs()
            .iter()
            .find(|s| s.name == "read_file")
            .expect("read_file spec not found");
        let path_input = read_file_spec
            .inputs
            .iter()
            .find(|i| i.name == "path")
            .expect("path input not found");
        assert!(path_input.required, "read_file path should be required");
    }
}
