const content = import.meta.glob<string>("./content/*.md", {
	eager: true,
	query: "?raw",
	import: "default",
});

function article(
	slug: string,
	title: string,
	description: string,
	group: string,
) {
	const markdown = content[`./content/${slug}.md`];
	if (markdown === undefined)
		throw new Error(`Missing documentation: ${slug}.md`);
	return { slug, title, description, group, markdown };
}

export const docs = [
	article(
		"introduction",
		"Introduction",
		"One description of your Rust types, shared across your stack.",
		"Getting started",
	),
	article(
		"installation",
		"Installation",
		"Install the CLI or select the library features your project needs.",
		"Getting started",
	),
	article(
		"quickstart",
		"Your first project",
		"Generate an ArkType schema from a validated Rust struct.",
		"Getting started",
	),
	article(
		"build-scripts",
		"Build scripts",
		"Generate types and offline SurrealQL during cargo build.",
		"Getting started",
	),
	article(
		"derives",
		"Derives & metadata",
		"Choose a pipeline and access registered type information at runtime.",
		"Rust types",
	),
	article(
		"serde",
		"Structs & enums",
		"Understand how serde representations flow into every output.",
		"Rust types",
	),
	article(
		"newtypes",
		"Newtypes & text codecs",
		"Brand values, validate constructors, and decode text representations.",
		"Rust types",
	),
	article(
		"naming",
		"Field naming",
		"Control TypeScript names, serde names, and stored database keys.",
		"Rust types",
	),
	article(
		"foreign-types",
		"Foreign types & links",
		"Map external crates and model linked SurrealDB records.",
		"Rust types",
	),
	article(
		"validation",
		"Validators",
		"Check values in Rust, generated TypeScript, and database assertions.",
		"Validation",
	),
	article(
		"morphs",
		"Morphs",
		"Normalize values before checking their constraints.",
		"Validation",
	),
	article(
		"validator-overrides",
		"Pipeline overrides",
		"Replace validators for a specific output pipeline.",
		"Validation",
	),
	article(
		"typesync",
		"Type generation",
		"Configure ArkType, Effect, Macroforge, Protobuf, and FlatBuffers.",
		"Type generation",
	),
	article(
		"output-layout",
		"Output layout",
		"Organize generated files, imports, and struct variants.",
		"Type generation",
	),
	article(
		"macroforge",
		"Macroforge output",
		"Configure derives, annotations, and custom macro imports.",
		"Type generation",
	),
	article(
		"schemasync",
		"Schema synchronization",
		"Inspect and apply schema changes or dump SurrealQL offline.",
		"Database",
	),
	article(
		"storage",
		"Storage & optional values",
		"Describe stored shapes and select NONE or NULL for absence.",
		"Database",
	),
	article(
		"table-rules",
		"Tables & field rules",
		"Configure indexes, permissions, relations, events, and field definitions.",
		"Database",
	),
	article(
		"mock-data",
		"Mock data",
		"Generate realistic values that satisfy your schema and validators.",
		"Database",
	),
	article(
		"workspaces",
		"Workspaces & scanning",
		"Share configuration across projects and resolve shared Rust types.",
		"Advanced",
	),
	article(
		"plugins",
		"WebAssembly plugins",
		"Extend output rules, synthetic types, and mock data generation.",
		"Advanced",
	),
	article(
		"cli",
		"CLI reference",
		"Commands and options for the full generation pipeline.",
		"Reference",
	),
	article(
		"configuration",
		"Configuration",
		"Configuration discovery, environment variables, and key settings.",
		"Reference",
	),
	article(
		"features",
		"Cargo features",
		"Select the functionality and generators compiled into your project.",
		"Reference",
	),
	article(
		"upgrading",
		"Upgrading to 0.6",
		"Move transforms to morphs and parse validators to text codecs.",
		"Reference",
	),
	article(
		"troubleshooting",
		"Troubleshooting",
		"Resolve common derive, generation, and schema issues.",
		"Reference",
	),
];

export const groups = [...new Set(docs.map((doc) => doc.group))];

export function searchDocs(query: string) {
	const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0)
		return docs.filter((doc) => doc.group === "Getting started");
	return docs
		.filter((doc) =>
			words.every((word) =>
				`${doc.title} ${doc.description} ${doc.markdown}`
					.toLowerCase()
					.includes(word),
			),
		)
		.sort(
			(left, right) =>
				Number(words.some((word) => right.title.toLowerCase().includes(word))) -
				Number(words.some((word) => left.title.toLowerCase().includes(word))),
		);
}
