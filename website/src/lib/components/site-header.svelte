<script lang="ts">
import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
import Menu from "@lucide/svelte/icons/menu";
import Moon from "@lucide/svelte/icons/moon";
import Search from "@lucide/svelte/icons/search";
import Sun from "@lucide/svelte/icons/sun";
import { onMount } from "svelte";
import Brand from "#lib/components/brand.svelte";
import DocsNav from "#lib/components/docs-nav.svelte";
import Button from "#lib/components/ui/button/button.svelte";
import * as Dialog from "#lib/components/ui/dialog/index.js";
import Input from "#lib/components/ui/input/input.svelte";
import * as Sheet from "#lib/components/ui/sheet/index.js";
import { searchDocs } from "#lib/docs.js";
import { goto } from "$app/navigation";
import { resolve } from "$app/paths";
import { page } from "$app/state";

let searchOpen = $state(false);
let menuOpen = $state(false);
let query = $state("");
let dark = $state(false);
const results = $derived(searchDocs(query));
onMount(() => {
	dark = document.documentElement.classList.contains("dark");
});

function toggleTheme() {
	dark = !dark;
	document.documentElement.classList.toggle("dark", dark);
	try {
		localStorage.setItem("evenframe-theme", dark ? "dark" : "light");
	} catch (error) {
		console.error("Could not save theme preference", error);
	}
}

function keyboard(event: KeyboardEvent) {
	if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
		event.preventDefault();
		searchOpen = !searchOpen;
	}
}

async function openFirstResult(event: SubmitEvent) {
	event.preventDefault();
	const first = results.at(0);
	if (!first) return;
	searchOpen = false;
	await goto(resolve("/docs/[slug]", { slug: first.slug }));
}
</script>

<svelte:window onkeydown={keyboard} />

<header class="sticky top-0 z-30 border-b bg-background/95 backdrop-blur">
	<div class="mx-auto flex h-16 max-w-[1440px] items-center gap-6 px-5 lg:px-8">
		<Brand />
		<span
			class="hidden rounded-md border bg-muted px-2 py-0.5 font-mono text-xs text-muted-foreground sm:block"
			>v0.6</span
		>
		<nav aria-label="Main" class="hidden items-center gap-6 text-sm md:flex">
			<a
				href={resolve("/docs/[slug]", { slug: "introduction" })}
				class={page.route.id?.startsWith("/docs")
					? "font-medium"
					: "text-muted-foreground hover:text-foreground"}
				>Documentation</a
			>
			<a
				href="https://docs.rs/evenframe"
				class="text-muted-foreground hover:text-foreground"
				>API reference</a
			>
		</nav>
		<div class="ml-auto flex items-center gap-2">
			<Dialog.Root
				bind:open={searchOpen}
				onOpenChange={(open) => {
					if (!open) query = "";
				}}
			>
				<Dialog.Trigger>
					{#snippet child({
						props,
					})}
						<Button
							{...props}
							variant="outline"
							aria-label="Search documentation"
							class="gap-2 text-muted-foreground md:w-56 md:justify-start"
						>
							<Search class="size-4" />
							<span class="hidden md:inline">Search documentation</span
							><kbd
								class="ml-auto hidden rounded border px-1.5 font-mono text-[10px] md:inline"
								>⌘ K</kbd
							>
						</Button>
					{/snippet}
				</Dialog.Trigger>
				<Dialog.Content class="gap-0 overflow-hidden p-0 sm:max-w-xl">
					<Dialog.Header class="border-b px-5 py-4 pr-12">
						<Dialog.Title>Search documentation</Dialog.Title>
						<Dialog.Description
							>Search guides, configuration, and reference
							articles.</Dialog.Description
						>
					</Dialog.Header>
					<form onsubmit={openFirstResult} class="p-4">
						<label for="doc-search" class="sr-only">Search query</label>
						<Input
							id="doc-search"
							type="search"
							bind:value={query}
							placeholder="What are you building?"
							autocomplete="off"
						/>
					</form>
					<p aria-live="polite" class="px-5 pb-2 text-xs text-muted-foreground">
						{query.trim() ? `${results.length} results` : "Start here"}
					</p>
					<nav
						aria-label="Search results"
						class="max-h-[50vh] overflow-y-auto p-2"
					>
						{#each results as doc (doc.slug)}
							<a
								href={resolve("/docs/[slug]", { slug: doc.slug })}
								onclick={() => {
									searchOpen = false;
								}}
								class="block rounded-lg px-3 py-3 hover:bg-accent focus-visible:bg-accent"
							>
								<span class="text-xs text-muted-foreground">{doc.group}</span
								><span class="mt-0.5 block font-medium">{doc.title}</span
								><span class="mt-1 block text-sm text-muted-foreground"
									>{doc.description}</span
								>
							</a>
						{:else}
							<div class="px-5 py-10 text-center">
								<p class="font-medium">No matching articles</p>
								<p class="mt-2 text-sm text-muted-foreground">
									Try a type name, command, or a shorter search.
								</p>
							</div>
						{/each}
					</nav>
					<div
						class="border-t bg-muted/40 px-5 py-3 text-xs text-muted-foreground"
					>
						Tab to browse · Enter to open · Escape to close
					</div>
				</Dialog.Content>
			</Dialog.Root>
			<Button
				variant="ghost"
				size="icon"
				aria-label={dark ? "Switch to light theme" : "Switch to dark theme"}
				onclick={toggleTheme}
			>
				{#if dark}
					<Sun class="size-4" />
				{:else}
					<Moon class="size-4" />
				{/if}
			</Button>
			<a
				href="https://github.com/rymskip/evenframe"
				class="hidden items-center gap-1 px-2 text-sm text-muted-foreground hover:text-foreground lg:flex"
				>GitHub <ArrowUpRight class="size-3.5" /></a
			>
			<Sheet.Root bind:open={menuOpen}>
				<Sheet.Trigger>
					{#snippet child({
						props,
					})}
						<Button
							{...props}
							variant="ghost"
							size="icon"
							aria-label="Open navigation"
							class="md:hidden"
							><Menu class="size-5" /></Button
						>
					{/snippet}
				</Sheet.Trigger>
				<Sheet.Content side="left" class="w-[min(85vw,22rem)]">
					<Sheet.Header>
						<Sheet.Title>Evenframe documentation</Sheet.Title>
						<Sheet.Description
							>Guides and reference for version 0.6.</Sheet.Description
						>
					</Sheet.Header>
					<div class="overflow-y-auto px-3 pb-8">
						<DocsNav
							onNavigate={() => {
								menuOpen = false;
							}}
						/><a
							href="https://docs.rs/evenframe"
							class="mt-6 block px-3 text-sm"
							>API reference ↗</a
						>
					</div>
				</Sheet.Content>
			</Sheet.Root>
		</div>
	</div>
</header>
