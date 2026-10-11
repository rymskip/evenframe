<script lang="ts">
import ArrowLeft from "@lucide/svelte/icons/arrow-left";
import ArrowRight from "@lucide/svelte/icons/arrow-right";
import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
import { copyCode } from "#lib/code-copy.util.js";
import { resolve } from "$app/paths";
import type { PageProps } from "./$types";

let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.doc.title} · Evenframe documentation</title>
	<meta name="description" content={data.doc.description}>
	<meta property="og:title" content={`${data.doc.title} · Evenframe`}>
	<meta property="og:description" content={data.doc.description}>
</svelte:head>

<main id="main-content" class="min-w-0 flex-1 px-6 py-10 lg:px-12 lg:py-12">
	<div class="mx-auto max-w-3xl">
		<div class="mb-6 flex items-center gap-2 text-xs text-muted-foreground">
			<span>Documentation</span><span aria-hidden="true">/</span
			><span>{data.doc.group}</span>
		</div>
		<h1 class="text-3xl font-semibold tracking-tight sm:text-4xl">
			{data.doc.title}
		</h1>
		<p class="mt-4 mb-8 text-lg leading-8 text-muted-foreground">
			{data.doc.description}
		</p>
		<div class="h-1 w-12 rounded-full bg-primary"></div>
		<details class="mt-8 rounded-lg border bg-muted/30 p-4 xl:hidden">
			<summary class="cursor-pointer text-sm font-medium">On this page</summary>
			<nav aria-label="On this page" class="mt-3 space-y-2">
				{#each data.headings as heading (heading.id)}
					<a
						href={`#${heading.id}`}
						class="block text-sm text-muted-foreground hover:text-foreground"
						>{heading.title}</a
					>
				{/each}
			</nav>
		</details>
		<article class="docs-prose" use:copyCode>
			{@html data.sanitizedHtml}
		</article>
		<div
			class="mt-12 flex flex-wrap gap-4 border-t pt-6 text-xs text-muted-foreground"
		>
			<a
				href={`https://github.com/rymskip/evenframe/edit/main/website/src/lib/content/${data.doc.slug}.md`}
				class="flex items-center gap-1 hover:text-foreground"
				>Edit this page <ArrowUpRight class="size-3" /></a
			>
			<span class="ml-auto">Evenframe 0.6</span>
		</div>
		<nav aria-label="Adjacent articles" class="mt-8 grid gap-4 sm:grid-cols-2">
			{#if data.previous}
				<a
					href={resolve("/docs/[slug]", { slug: data.previous.slug })}
					class="group rounded-lg border bg-card p-5 transition-colors hover:border-ring"
					><span class="flex items-center gap-2 text-xs text-muted-foreground"
						><ArrowLeft class="size-3" />
						Previous</span
					><span class="mt-2 block font-medium">{data.previous.title}</span></a
				>
			{:else}
				<div></div>
			{/if}
			{#if data.next}
				<a
					href={resolve("/docs/[slug]", { slug: data.next.slug })}
					class="group rounded-lg border bg-card p-5 text-right transition-colors hover:border-ring"
					><span
						class="flex items-center justify-end gap-2 text-xs text-muted-foreground"
						>Next <ArrowRight class="size-3" /></span
					><span class="mt-2 block font-medium">{data.next.title}</span></a
				>
			{/if}
		</nav>
	</div>
</main>

<aside class="sticky top-16 hidden h-fit w-56 shrink-0 py-12 pr-8 xl:block">
	<nav aria-label="On this page">
		<h2 class="mb-4 text-xs font-semibold">On this page</h2>
		<ul class="space-y-3 border-l pl-4">
			{#each data.headings as heading (heading.id)}
				<li>
					<a
						href={`#${heading.id}`}
						class="block text-xs leading-5 text-muted-foreground hover:text-foreground"
						>{heading.title}</a
					>
				</li>
			{/each}
		</ul>
	</nav>
	<div class="mt-10 rounded-lg border bg-accent/40 p-4">
		<p class="text-sm font-medium">Keep it in Rust.</p>
		<p class="mt-2 text-xs leading-5 text-muted-foreground">
			Types, validation, and storage share one definition.
		</p>
		<a
			href="https://docs.rs/evenframe"
			class="mt-3 flex items-center gap-1 text-xs font-medium"
			>Rust API <ArrowUpRight class="size-3" /></a
		>
	</div>
</aside>
