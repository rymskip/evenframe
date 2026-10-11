<script lang="ts">
import { asset, resolve } from "$app/paths";
</script>

<a
	href={resolve("/")}
	aria-label="Evenframe home"
	class="inline-flex items-center gap-2.5 font-semibold tracking-tight"
>
	<img
		src={asset("favicon.svg")}
		alt=""
		width="32"
		height="32"
		class="size-8 rounded-md"
	>
	<span class="text-lg">evenframe</span>
</a>
