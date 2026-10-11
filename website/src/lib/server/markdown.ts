import GithubSlugger from "github-slugger";
import { Marked, Renderer } from "marked";
import sanitizeHtml from "sanitize-html";
import { createHighlighter } from "shiki";

const highlighter = await createHighlighter({
	themes: ["github-light", "github-dark"],
	langs: ["rust", "toml", "bash", "typescript", "json"],
});

export function renderMarkdown(markdown: string) {
	const headings: { id: string; title: string }[] = [];
	const slugger = new GithubSlugger();
	const renderer = new Renderer();
	renderer.heading = function ({ text, depth, tokens }) {
		const id = slugger.slug(text);
		headings.push({ id, title: text });
		return `<h${depth} id="${id}"><a class="heading-link" href="#${id}">${this.parser.parseInline(tokens)}</a></h${depth}>`;
	};
	renderer.code = ({ text, lang }) => {
		const language = lang === "sh" ? "bash" : lang || "text";
		const highlighted = highlighter.codeToHtml(text, {
			lang: language,
			themes: { light: "github-light", dark: "github-dark" },
			defaultColor: false,
		});
		return `<div class="code-frame"><div class="code-toolbar"><span>${language}</span><button type="button" data-copy-code aria-label="Copy ${language} code">Copy</button></div>${highlighted}</div>`;
	};
	renderer.table = function (token) {
		return `<div class="table-frame">${Renderer.prototype.table.call(this, token)}</div>`;
	};
	const sanitizedHtml = sanitizeHtml(
		new Marked({ renderer }).parse(markdown, { async: false }),
		{
			allowedTags: [...sanitizeHtml.defaults.allowedTags, "button"],
			allowedAttributes: {
				a: ["href", "class"],
				h2: ["id"],
				h3: ["id"],
				div: ["class"],
				pre: ["class", "style", "tabindex"],
				span: ["class", "style"],
				button: ["type", "data-copy-code", "aria-label"],
			},
			allowedStyles: {
				"*": Object.fromEntries(
					[
						"--shiki-light",
						"--shiki-light-bg",
						"--shiki-dark",
						"--shiki-dark-bg",
					].map((property) => [property, [/^#[0-9a-f]{3,8}$/i]]),
				),
			},
		},
	);
	return { sanitizedHtml, headings };
}
