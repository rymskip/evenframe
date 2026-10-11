<script lang="ts">
import { docs, groups } from "#lib/docs.js";
import { resolve } from "$app/paths";
import { page } from "$app/state";

let { onNavigate }: { onNavigate?: () => void } = $props();
</script>

<nav aria-label="Documentation" class="space-y-7">
	{#each groups as group (group)}
		<div>
			<h2
				class="mb-2 px-3 text-xs font-semibold tracking-wider text-muted-foreground uppercase"
			>
				{group}
			</h2>
			<ul class="space-y-0.5">
				{#each docs.filter((doc) => doc.group === group) as doc (doc.slug)}
					<li>
						<a
							href={resolve("/docs/[slug]", { slug: doc.slug })}
							onclick={onNavigate}
							aria-current={page.params.slug === doc.slug ? "page" : undefined}
							class={[
								"block rounded-md border-l-2 px-3 py-2 text-sm transition-colors",
								page.params.slug === doc.slug
									? "border-primary bg-accent font-medium text-accent-foreground"
									: "border-transparent text-muted-foreground hover:bg-muted hover:text-foreground",
							]}
							>{doc.title}</a
						>
					</li>
				{/each}
			</ul>
		</div>
	{/each}
</nav>
