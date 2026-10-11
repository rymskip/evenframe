import { expect, test } from "@playwright/test";

test("homepage links lead into the complete documentation navigation", async ({
	page,
}) => {
	const errors: string[] = [];
	page.on("pageerror", (error) => errors.push(error.message));
	page.on("console", (message) => {
		if (message.type() === "error") errors.push(message.text());
	});
	await page.goto("/");
	await expect(page.getByRole("heading", { level: 1 })).toContainText(
		"Your Rust types.",
	);
	await page.getByRole("link", { name: "Get started" }).click();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Your first project",
	);
	const navigation = page.getByRole("navigation", {
		name: "Documentation",
		exact: true,
	});
	const titles = await navigation.getByRole("link").allTextContents();
	expect(titles.length).toBe(26);
	for (const title of titles) {
		await navigation.getByRole("link", { name: title, exact: true }).click();
		await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
		await expect(
			navigation.getByRole("link", { name: title, exact: true }),
		).toHaveAttribute("aria-current", "page");
		await expect(page.locator("article")).not.toBeEmpty();
		await expect(page).toHaveTitle(`${title} · Evenframe documentation`);
	}
	expect(errors).toEqual([]);
});

test("search matches article bodies, handles empty results, and supports keyboard entry", async ({
	page,
}) => {
	await page.goto("/");
	await page
		.getByRole("button", { name: "Search documentation", exact: true })
		.click();
	const dialog = page.getByRole("dialog");
	await dialog
		.getByRole("searchbox", { name: "Search query" })
		.fill("option_none");
	await dialog.getByRole("link", { name: /Storage & optional values/ }).click();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Storage & optional values",
	);
	await expect(dialog).not.toBeVisible();
	await page.keyboard.press("Control+k");
	await dialog.getByRole("searchbox").fill("no-such-documentation-term");
	await expect(dialog.getByText("No matching articles")).toBeVisible();
	await dialog.getByRole("searchbox").fill("validators");
	await dialog.getByRole("searchbox").press("Enter");
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Validators",
	);
	await page.keyboard.press("Control+k");
	await page.keyboard.press("Escape");
	await expect(dialog).not.toBeVisible();
});

test("theme preference persists and code is copied through the visible control", async ({
	page,
}) => {
	await page.goto("/");
	await page.getByRole("button", { name: "Switch to dark theme" }).click();
	await expect(page.locator("html")).toHaveClass("dark");
	await expect(page.locator(".shiki")).toHaveCSS(
		"background-color",
		"rgb(36, 41, 46)",
	);
	await expect(page.locator(".shiki .line span").first()).toHaveCSS(
		"color",
		"rgb(249, 117, 131)",
	);
	await page.reload();
	await expect(page.locator("html")).toHaveClass("dark");
	await page.getByRole("button", { name: "Switch to light theme" }).click();
	await expect(page.locator("html")).not.toHaveClass("dark");
	await expect(page.locator(".shiki")).toHaveCSS(
		"background-color",
		"rgb(255, 255, 255)",
	);
	await expect(page.locator(".shiki .line span").first()).toHaveCSS(
		"color",
		"rgb(215, 58, 73)",
	);
	await page.getByRole("button", { name: "Copy rust code" }).click();
	await expect(page.getByRole("button", { name: "Copy rust code" })).toHaveText(
		"Copied",
	);
	expect(await page.evaluate(() => navigator.clipboard.readText())).toContain(
		"pub struct Profile",
	);
});

test("mobile navigation and table of contents remain usable without page overflow", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto("/");
	await page.getByRole("button", { name: "Open navigation" }).click();
	const dialog = page.getByRole("dialog");
	await dialog.getByRole("link", { name: "Validators", exact: true }).click();
	await expect(dialog).not.toBeVisible();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Validators",
	);
	await page.locator("summary").click();
	await page
		.getByRole("navigation", { name: "On this page" })
		.getByRole("link", { name: "Portable regular expressions" })
		.click();
	await expect(page).toHaveURL(/#portable-regular-expressions$/);
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth <= window.innerWidth,
		),
	).toBe(true);
	await page.getByRole("link", { name: /Next Morphs/ }).click();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText("Morphs");
});

test("Workers serves nested static pages and a real 404", async ({ page }) => {
	await page.goto("/");
	await page.getByRole("link", { name: "Read the docs" }).click();
	await page
		.getByRole("navigation", { name: "Documentation", exact: true })
		.getByRole("link", { name: "Cargo features" })
		.click();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Cargo features",
	);
	await page.reload();
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Cargo features",
	);
	const response = await page.goto("/docs/does-not-exist/");
	expect(response?.status()).toBe(404);
	await expect(page.getByRole("heading", { level: 1 })).toHaveText(
		"Page not found",
	);
	await page.getByRole("link", { name: "Return to Evenframe" }).click();
	await expect(page.getByRole("heading", { level: 1 })).toContainText(
		"Your Rust types.",
	);
});
