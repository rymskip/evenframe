<script lang="ts">
import DocsNav from "#lib/components/docs-nav.svelte";
import type { LayoutProps } from "./$types";

let { children }: LayoutProps = $props();
</script>

<div class="mx-auto flex max-w-[1440px]">
	<aside
		class="sticky top-16 hidden h-[calc(100dvh-4rem)] w-64 shrink-0 overflow-y-auto border-r px-5 py-8 md:block lg:w-72 lg:px-6"
	>
		<DocsNav />
	</aside>
	{@render children()}
</div>
