import { error } from "@sveltejs/kit";
import { docs } from "#lib/docs.js";
import { renderMarkdown } from "#lib/server/markdown.js";
import type { EntryGenerator, PageServerLoad } from "./$types";

export const entries: EntryGenerator = () =>
	docs.map((doc) => ({ slug: doc.slug }));

export const load: PageServerLoad = ({ params }) => {
	const index = docs.findIndex((doc) => doc.slug === params.slug);
	const doc = docs.at(index);
	if (index < 0 || !doc) error(404, "Documentation page not found");
	return {
		doc: {
			slug: doc.slug,
			title: doc.title,
			description: doc.description,
			group: doc.group,
		},
		...renderMarkdown(doc.markdown),
		previous: index > 0 ? docs.at(index - 1) : undefined,
		next: docs.at(index + 1),
	};
};
