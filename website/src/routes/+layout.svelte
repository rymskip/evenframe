<script lang="ts">
import SiteHeader from "#lib/components/site-header.svelte";
import { asset } from "$app/paths";
import "./layout.css";
import type { LayoutProps } from "./$types";

let { data, children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={asset("favicon.svg")}>
	<meta name="theme-color" content="#edcd36">
	{#if data.canonicalUrl}
		<link rel="canonical" href={data.canonicalUrl}>
		<meta property="og:url" content={data.canonicalUrl}>
	{/if}
</svelte:head>

<a
	href="#main-content"
	class="sr-only fixed top-2 left-2 z-50 rounded bg-primary p-3 text-primary-foreground focus:not-sr-only"
	>Skip to content</a
>
<SiteHeader />
{@render children()}
<footer class="border-t">
	<div
		class="mx-auto flex max-w-[1440px] flex-wrap items-center justify-between gap-4 px-6 py-7 text-xs text-muted-foreground lg:px-8"
	>
		<p>Evenframe · Rust types, everywhere.</p>
		<div class="flex gap-5">
			<a
				href="https://github.com/rymskip/evenframe"
				class="hover:text-foreground"
				>Source on GitHub</a
			><a
				href="https://github.com/rymskip/evenframe/blob/main/LICENSE"
				class="hover:text-foreground"
				>MIT license</a
			>
		</div>
	</div>
</footer>
