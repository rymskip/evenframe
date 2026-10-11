<script lang="ts">
import Button from "#lib/components/ui/button/button.svelte";
import { resolve } from "$app/paths";
import { page } from "$app/state";
</script>

<svelte:head
	><title>{page.status} · Evenframe</title>
	<meta name="robots" content="noindex"></svelte:head
>
<main
	id="main-content"
	class="mx-auto min-h-[65vh] max-w-xl px-6 py-24 text-center"
>
	<p class="font-mono text-sm text-muted-foreground">{page.status}</p>
	<h1 class="mt-5 text-4xl font-semibold tracking-tight">
		{page.status === 404 ? "Page not found" : "Something went wrong"}
	</h1>
	<p class="mt-5 mb-8 text-muted-foreground">
		{page.error?.message ?? "Please try again."}
	</p>
	<Button href={resolve("/docs/[slug]", { slug: "introduction" })}
		>Back to documentation</Button
	>
</main>
