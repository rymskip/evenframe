import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

function basePath(): "" | `/${string}` {
	if (!process.env.BASE_PATH) return "";
	if (
		!process.env.BASE_PATH.startsWith("/") ||
		process.env.BASE_PATH.endsWith("/")
	) {
		throw new Error("BASE_PATH must start with / and have no trailing slash");
	}
	return `/${process.env.BASE_PATH.substring(1)}`;
}

export default defineConfig({
	ssr: { noExternal: ["sanitize-html", "htmlparser2"] },
	plugins: [
		tailwindcss(),
		sveltekit({
			adapter: adapter(),
			paths: { base: basePath() },
		}),
	],
});

import process from "node:process";
