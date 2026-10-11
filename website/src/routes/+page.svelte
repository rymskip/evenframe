<script lang="ts">
import ArrowRight from "@lucide/svelte/icons/arrow-right";
import Braces from "@lucide/svelte/icons/braces";
import Database from "@lucide/svelte/icons/database";
import Layers from "@lucide/svelte/icons/layers";
import ShieldCheck from "@lucide/svelte/icons/shield-check";
import { copyCode } from "#lib/code-copy.util.js";
import Button from "#lib/components/ui/button/button.svelte";
import { resolve } from "$app/paths";
import type { PageProps } from "./$types";

let { data }: PageProps = $props();
const features = [
	{
		icon: Braces,
		number: "01",
		title: "TypeScript, in sync",
		description:
			"Generate ArkType, Effect, and Macroforge definitions from the types your backend already owns.",
		slug: "typesync",
	},
	{
		icon: Database,
		number: "02",
		title: "A schema that follows",
		description:
			"Compare and synchronize SurrealDB schemas. Or generate the SurrealQL offline, right in your build.",
		slug: "schemasync",
	},
	{
		icon: ShieldCheck,
		number: "03",
		title: "Validation that travels",
		description:
			"Declare constraints once. Check Rust input, generated TypeScript, and stored values from the same rules.",
		slug: "validation",
	},
	{
		icon: Layers,
		number: "04",
		title: "Data to build with",
		description:
			"Generate mock records shaped by your formats, relationships, validators, and normalization rules.",
		slug: "mock-data",
	},
];
</script>

<svelte:head
	><title>Evenframe · Rust types, everywhere</title>
	<meta
		name="description"
		content="Generate TypeScript types, synchronize SurrealDB schemas, and validate data from one source of truth: your Rust types."
	>
	<meta property="og:title" content="Evenframe · Rust types, everywhere">
	<meta
		property="og:description"
		content="Type generation, schema synchronization, and validation from your Rust structs and enums."
	></svelte:head
>

<main id="main-content">
	<section class="relative overflow-hidden border-b">
		<div
			aria-hidden="true"
			class="pointer-events-none absolute -top-24 right-0 size-[38rem] rounded-full bg-primary/10 blur-3xl"
		></div>
		<div
			class="relative mx-auto grid max-w-7xl items-center gap-14 px-6 py-20 lg:grid-cols-2 lg:px-8 lg:py-28"
		>
			<div>
				<div
					class="mb-7 inline-flex items-center gap-2 rounded-full border bg-card px-3 py-1.5 text-xs text-muted-foreground"
				>
					<span class="size-1.5 rounded-full bg-primary"></span>Open source.
					Rust first. Version 0.6.
				</div>
				<h1
					class="max-w-xl text-5xl leading-[1.1] font-semibold tracking-tight sm:text-6xl"
				>
					Your Rust types.<br>
					<span class="relative inline-block"
						>Your whole stack.<span
							aria-hidden="true"
							class="absolute -bottom-2 left-0 -z-10 h-4 w-full -rotate-1 bg-primary"
						></span></span
					>
				</h1>
				<p class="mt-8 max-w-lg text-lg leading-8 text-muted-foreground">
					TypeScript definitions. Database schemas. Validation. Mock data. One
					source of truth, from the structs and enums you already write.
				</p>
				<div class="mt-9 flex flex-wrap gap-3">
					<Button
						href={resolve("/docs/[slug]", { slug: "quickstart" })}
						size="lg"
						class="h-11 gap-3 px-5"
						>Get started <ArrowRight class="size-4" /></Button
					><Button
						href={resolve("/docs/[slug]", { slug: "introduction" })}
						variant="outline"
						size="lg"
						class="h-11 px-5"
						>Read the docs</Button
					>
				</div>
				<p class="mt-5 font-mono text-xs text-muted-foreground">
					cargo install evenframe
				</p>
			</div>
			<div class="min-w-0">
				<div
					class="rounded-xl border bg-card p-1.5 shadow-lg shadow-foreground/5"
				>
					<div
						class="flex items-center justify-between px-4 py-3 text-xs text-muted-foreground"
					>
						<span class="font-mono">src/profile.rs</span
						><span>One definition</span>
					</div>
					<div
						class="docs-prose [&_.code-frame]:m-0 [&_.code-frame]:border-0"
						use:copyCode
					>
						{@html data.sanitizedHtml}
					</div>
				</div>
				<div
					class="mx-auto mt-5 flex flex-wrap justify-center gap-2 text-xs text-muted-foreground"
				>
					{#each [
						"Rust validation",
						"TypeScript types",
						"SurrealDB schema",
					] as output (output)}
						<span class="rounded-md border bg-card px-3 py-2">{output}</span>
					{/each}
				</div>
			</div>
		</div>
	</section>
	<section class="mx-auto max-w-7xl px-6 py-16 lg:px-8 lg:py-20">
		<div class="mb-10 flex flex-wrap items-end justify-between gap-4">
			<div>
				<p
					class="mb-3 text-xs font-semibold tracking-widest text-muted-foreground uppercase"
				>
					Less repetition. More building.
				</p>
				<h2 class="text-3xl font-semibold tracking-tight">
					Define once. Use everywhere.
				</h2>
			</div>
			<p class="max-w-sm text-sm leading-6 text-muted-foreground">
				Start with just the derive. Add generation, schema sync, and plugins
				when you need them.
			</p>
		</div>
		<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
			{#each features as feature (feature.slug)}
				<a
					href={resolve("/docs/[slug]", { slug: feature.slug })}
					class="group rounded-xl border bg-card p-6 transition-colors hover:border-ring"
					><div class="mb-8 flex items-center justify-between">
						<feature.icon class="size-6 text-foreground" />
						<span class="font-mono text-xs text-muted-foreground"
							>{feature.number}</span
						>
					</div>
					<h3 class="text-base font-semibold">{feature.title}</h3>
					<p class="mt-3 text-sm leading-6 text-muted-foreground">
						{feature.description}
					</p>
					<span class="mt-6 flex items-center gap-2 text-xs font-medium"
						>Explore
						<ArrowRight
							class="size-3 transition-transform group-hover:translate-x-1"
						/></span
					></a
				>
			{/each}
		</div>
	</section>
	<section class="border-t bg-muted/40">
		<div
			class="mx-auto grid max-w-7xl gap-10 px-6 py-16 lg:grid-cols-[1fr_2fr] lg:px-8"
		>
			<div>
				<p class="mb-3 font-mono text-xs text-muted-foreground">
					CHOOSE YOUR ENTRY POINT
				</p>
				<h2 class="text-3xl font-semibold tracking-tight">
					As much framework<br>as you need.
				</h2>
				<p class="mt-4 text-sm leading-6 text-muted-foreground">
					Independent features. Shared type information.
				</p>
			</div>
			<div class="grid gap-6 sm:grid-cols-3">
				{#each [
					{
						title: "At runtime",
						text: "Validate input and inspect your types. No database required.",
						slug: "derives",
					},
					{
						title: "At build time",
						text: "Generate definitions and offline SurrealQL with Cargo.",
						slug: "build-scripts",
					},
					{
						title: "From the CLI",
						text: "Run the full pipeline, inspect changes, and create mock data.",
						slug: "cli",
					},
				] as workflow (workflow.slug)}
					<a
						href={resolve("/docs/[slug]", { slug: workflow.slug })}
						class="border-t-2 border-primary pt-5"
						><h3 class="font-semibold">{workflow.title}</h3>
						<p class="mt-3 text-sm leading-6 text-muted-foreground">
							{workflow.text}
						</p>
						<ArrowRight class="mt-5 size-4" /></a
					>
				{/each}
			</div>
		</div>
	</section>
</main>
