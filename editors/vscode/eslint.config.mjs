import eslint from "@eslint/js";
import typescriptEslint from "@typescript-eslint/eslint-plugin";
import typescriptParser from "@typescript-eslint/parser";
import globals from "globals";

const languageOptions = {
  ecmaVersion: "latest",
  sourceType: "module",
  globals: {
    ...globals.es2020,
    ...globals.node,
  },
};

export default [
  {
    ignores: ["out/**", "node_modules/**", "media/**", "*.js"],
  },
  {
    files: ["src/**/*.ts"],
    languageOptions: {
      ...languageOptions,
      parser: typescriptParser,
    },
    plugins: {
      "@typescript-eslint": typescriptEslint,
    },
    rules: {
      ...eslint.configs.recommended.rules,
      ...typescriptEslint.configs.recommended.rules,
      "no-undef": "off",
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-non-null-assertion": "off",
      "@typescript-eslint/explicit-module-boundary-types": "off",
    },
  },
  {
    files: ["scripts/**/*.{mjs,cjs}"],
    // The legacy root config used this parser for scripts too. Preserve that
    // behavior so the existing no-var-requires exception remains meaningful.
    languageOptions: {
      ...languageOptions,
      parser: typescriptParser,
    },
    plugins: {
      "@typescript-eslint": typescriptEslint,
    },
    rules: {
      ...eslint.configs.recommended.rules,
      "@typescript-eslint/no-var-requires": "error",
    },
  },
];
