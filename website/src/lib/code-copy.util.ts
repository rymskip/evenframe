export function copyCode(container: HTMLElement) {
	async function copy(event: MouseEvent) {
		if (!(event.target instanceof Element)) return;
		const button = event.target.closest("button[data-copy-code]");
		if (!(button instanceof HTMLButtonElement)) return;
		const code = button.closest(".code-frame")?.querySelector("code");
		if (!code?.textContent) return;
		try {
			await navigator.clipboard.writeText(code.textContent);
			button.textContent = "Copied";
		} catch (error) {
			console.error("Could not copy code to clipboard", error);
			button.textContent = "Copy failed";
		}
	}
	container.addEventListener("click", copy);
	return { destroy: () => container.removeEventListener("click", copy) };
}
