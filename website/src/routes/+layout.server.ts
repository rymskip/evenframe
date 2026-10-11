import process from "node:process";
import type { LayoutServerLoad } from "./$types";

export const load: LayoutServerLoad = ({ url }) => {
	if (!process.env.SITE_URL) return { canonicalUrl: null };
	const site = new URL(process.env.SITE_URL);
	if (
		!["https:", "http:"].includes(site.protocol) ||
		site.username ||
		site.password
	) {
		throw new Error("SITE_URL must be an HTTP(S) URL without credentials");
	}
	return { canonicalUrl: new URL(url.pathname, site.origin).href };
};
