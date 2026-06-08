pub fn compile_prompt(message: &str) -> String {
    format!(
        "System:\nYou are Caravan's local mock assistant.\n\nUser:\n{}\n\nContext:\nNo external context is available in this POC.\n\nOutput:\nRespond with a deterministic mock response.",
        message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_prompt_exact_template() {
        assert_eq!(
            compile_prompt("hello"),
            "System:\nYou are Caravan's local mock assistant.\n\nUser:\nhello\n\nContext:\nNo external context is available in this POC.\n\nOutput:\nRespond with a deterministic mock response."
        );
    }

    #[test]
    fn compile_prompt_contains_required_sections() {
        let result = compile_prompt("hello");
        assert!(result.contains("System:"));
        assert!(result.contains("User:"));
        assert!(result.contains("Context:"));
        assert!(result.contains("Output:"));
        assert!(result.contains("hello"));
    }
}
