import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: "./tests",
	workers: 1,
	use: {
		baseURL: process.env.PLAYWRIGHT_BASE_URL || "http://localhost:4173",
		permissions: ["clipboard-read", "clipboard-write"],
		trace: "retain-on-failure",
	},
	webServer: process.env.PLAYWRIGHT_BASE_URL
		? undefined
		: {
				command: "deno task serve --port 4173",
				url: "http://localhost:4173",
				timeout: 60_000,
			},
});

import process from "node:process";
