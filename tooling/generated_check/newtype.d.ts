// Macroforge expands `$Newtype<T>` to a type branded over `T`; declaring it
// here lets deno check the generated declarations outside macroforge.
declare const newtype: unique symbol;

declare global {
  type $Newtype<T> = T & { readonly [newtype]: true };
}

export {};
