<script lang="ts">
import { copyCode } from "#lib/code-copy.util.js";
import Button from "#lib/components/ui/button/button.svelte";
import { resolve } from "$app/paths";
import type { PageProps } from "./$types";

let { data }: PageProps = $props();
const features = [
	{
		title: "Type generation",
		description:
			"Writes ArkType, Effect, and Macroforge definitions for your Rust structs and enums.",
		slug: "typesync",
	},
	{
		title: "Schema synchronization",
		description:
			"Compares your types with a SurrealDB database and applies the differences, or writes the SurrealQL without connecting.",
		slug: "schemasync",
	},
	{
		title: "Validation",
		description:
			"Validators on a field run when Rust deserializes it, in the generated TypeScript, and as database assertions.",
		slug: "validation",
	},
	{
		title: "Mock data",
		description:
			"Generates test records that satisfy your field types, relations, and validators.",
		slug: "mock-data",
	},
];
const workflows = [
	{
		title: "Derive only",
		text: "Validated deserialization and runtime type information. No database or code generation.",
		slug: "derives",
	},
	{
		title: "Build script",
		text: "Generate TypeScript and SurrealQL files on every cargo build.",
		slug: "build-scripts",
	},
	{
		title: "CLI",
		text: "Generate files, synchronize a live database, and create mock data.",
		slug: "cli",
	},
];
</script>

<svelte:head
	><title>Evenframe · TypeScript and SurrealDB schemas from Rust types</title>
	<meta
		name="description"
		content="Generate TypeScript types, synchronize SurrealDB schemas, and validate data from your Rust structs and enums."
	>
	<meta
		property="og:title"
		content="Evenframe · TypeScript and SurrealDB schemas from Rust types"
	>
	<meta
		property="og:description"
		content="Type generation, schema synchronization, and validation from your Rust structs and enums."
	></svelte:head
>

<main id="main-content">
	<section class="border-b">
		<div
			class="mx-auto grid max-w-6xl items-start gap-12 px-6 py-16 lg:grid-cols-2 lg:px-8 lg:py-20"
		>
			<div>
				<h1 class="text-3xl font-semibold tracking-tight sm:text-4xl">
					Rust types as the source of truth
				</h1>
				<p class="mt-6 max-w-lg text-lg leading-8 text-muted-foreground">
					Evenframe reads the Rust structs and enums in your project and
					generates TypeScript types, SurrealDB schemas, validation, and mock
					data from them.
				</p>
				<div class="mt-8 flex flex-wrap gap-3">
					<Button href={resolve("/docs/[slug]", { slug: "quickstart" })}
						>Get started</Button
					><Button
						href={resolve("/docs/[slug]", { slug: "introduction" })}
						variant="outline"
						>Read the docs</Button
					>
				</div>
				<p class="mt-6 font-mono text-sm text-muted-foreground">
					cargo install evenframe
				</p>
			</div>
			<div class="min-w-0">
				<p class="mb-2 font-mono text-xs text-muted-foreground">
					src/profile.rs
				</p>
				<div class="docs-prose [&_.code-frame]:my-0" use:copyCode>
					{@html data.sanitizedHtml}
				</div>
			</div>
		</div>
	</section>
	<section class="mx-auto max-w-6xl px-6 py-14 lg:px-8">
		<h2 class="text-2xl font-semibold tracking-tight">What it does</h2>
		<div class="mt-8 grid gap-x-10 gap-y-8 sm:grid-cols-2">
			{#each features as feature (feature.slug)}
				<div>
					<a
						href={resolve("/docs/[slug]", { slug: feature.slug })}
						class="font-medium underline decoration-primary decoration-2 underline-offset-4 hover:decoration-foreground"
						>{feature.title}</a
					>
					<p class="mt-2 text-sm leading-6 text-muted-foreground">
						{feature.description}
					</p>
				</div>
			{/each}
		</div>
	</section>
	<section class="border-t">
		<div class="mx-auto max-w-6xl px-6 py-14 lg:px-8">
			<h2 class="text-2xl font-semibold tracking-tight">Ways to use it</h2>
			<p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
				Each part works on its own. A project can use the derive at runtime, a
				build script for TypeScript, and the CLI for its database.
			</p>
			<div class="mt-8 grid gap-x-10 gap-y-8 sm:grid-cols-3">
				{#each workflows as workflow (workflow.slug)}
					<div>
						<a
							href={resolve("/docs/[slug]", { slug: workflow.slug })}
							class="font-medium underline decoration-primary decoration-2 underline-offset-4 hover:decoration-foreground"
							>{workflow.title}</a
						>
						<p class="mt-2 text-sm leading-6 text-muted-foreground">
							{workflow.text}
						</p>
					</div>
				{/each}
			</div>
		</div>
	</section>
</main>
