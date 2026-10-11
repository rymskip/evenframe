import { renderMarkdown } from "#lib/server/markdown.js";

export function load() {
	return renderMarkdown(
		"```rust\nuse evenframe::Evenframe;\nuse serde::Serialize;\n\n#[derive(Debug, Clone, Serialize, Evenframe)]\npub struct Profile {\n    #[morphs(trim)]\n    #[validators(non_empty, max_length = 80)]\n    pub display_name: String,\n    #[validators(email)]\n    pub email: String,\n}\n```",
	);
}
