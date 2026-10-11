import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

export type WithElementRef<
	Props,
	Element extends HTMLElement = HTMLElement,
> = Props & {
	ref?: Element | null;
};

export type WithoutChildrenOrChild<Props> = Omit<Props, "children" | "child">;
