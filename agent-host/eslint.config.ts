// The agent host's lint policy: typescript-eslint's strictest type-checked sets, every rule an
// error, and the rules below on top. As in the Rust crates, a rule is never turned off to make
// an error go away; a line that truly needs an exception carries an
// `eslint-disable-next-line <rule> -- <reason>` comment, which `reportUnusedDisableDirectives`
// retires once the line stops tripping it. Options are written only where they differ from
// the rule's default.

import js from "@eslint/js";
import stylistic from "@stylistic/eslint-plugin";
import { defineConfig } from "eslint/config";
import simpleImportSort from "eslint-plugin-simple-import-sort";
import unicorn from "eslint-plugin-unicorn";
import tseslint from "typescript-eslint";

export default defineConfig(
  { ignores: ["dist/"] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  stylistic.configs.customize({
    quotes: "double",
    semi: true,
    jsx: false,
    braceStyle: "1tbs",
    arrowParens: true,
  }),
  {
    linterOptions: {
      reportUnusedDisableDirectives: "error",
      reportUnusedInlineConfigs: "error",
    },
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "simple-import-sort": simpleImportSort,
      unicorn,
    },
    rules: {
      // Types are proven, never asserted: no `as`, no `<T>x` (the presets already forbid `!`
      // and `any`). Untrusted input is parsed (zod), and a value whose type can't be proven
      // is narrowed by a check.
      "@typescript-eslint/consistent-type-assertions": ["error", { assertionStyle: "never" }],
      "@typescript-eslint/ban-ts-comment": ["error", { "ts-expect-error": true }],

      // Every branch of a union is handled, and a condition is a boolean, not a truthy value.
      "@typescript-eslint/switch-exhaustiveness-check": ["error", {
        allowDefaultCaseForExhaustiveSwitch: false,
        requireDefaultForNonUnion: true,
      }],
      "@typescript-eslint/strict-boolean-expressions": ["error", {
        allowString: false,
        allowNumber: false,
      }],

      // Promises are awaited, returned or handed to something that handles them.
      "@typescript-eslint/no-floating-promises": ["error", { checkThenables: true }],
      "@typescript-eslint/promise-function-async": "error",
      "@typescript-eslint/return-await": ["error", "always"],
      "@typescript-eslint/require-array-sort-compare": "error",

      // What a function returns is written down, so a change to it is a change to its type.
      "@typescript-eslint/explicit-function-return-type": ["error", { allowExpressions: true }],
      "@typescript-eslint/explicit-module-boundary-types": "error",
      // Function-typed properties are checked contravariantly; methods aren't.
      "@typescript-eslint/method-signature-style": "error",
      "@typescript-eslint/prefer-readonly": "error",
      "@typescript-eslint/consistent-type-imports": "error",
      "@typescript-eslint/consistent-type-exports": "error",
      "@typescript-eslint/no-import-type-side-effects": "error",
      "@typescript-eslint/no-shadow": "error",
      "@typescript-eslint/no-unused-vars": ["error", {
        args: "all",
        argsIgnorePattern: "^_",
        varsIgnorePattern: "^_",
      }],
      "@typescript-eslint/no-useless-empty-export": "error",

      // Plain JavaScript hygiene.
      "eqeqeq": "error",
      "curly": "error",
      "no-console": "error",
      "no-labels": "error",
      "no-param-reassign": ["error", { props: true }],
      "no-nested-ternary": "error",
      "no-implicit-coercion": "error",
      "no-else-return": ["error", { allowElseIf: false }],
      "no-lonely-if": "error",
      "no-useless-return": "error",
      "no-useless-concat": "error",
      "no-useless-rename": "error",
      "no-return-assign": ["error", "always"],
      "no-sequences": "error",
      "no-void": "error",
      "no-bitwise": "error",
      "no-plusplus": ["error", { allowForLoopAfterthoughts: true }],
      "no-restricted-syntax": ["error",
        { selector: "ForInStatement", message: "Use Object.keys/entries or a Map." },
      ],
      "object-shorthand": "error",
      "prefer-const": "error",
      "prefer-template": "error",
      "prefer-object-spread": "error",
      "prefer-arrow-callback": "error",
      "default-case-last": "error",
      "radix": "error",
      "yoda": "error",
      // One file, one job: the same 500-line limit script/hygiene holds the Rust files to.
      "max-lines": ["error", 500],
      "max-depth": ["error", 4],
      "max-params": ["error", 4],

      "simple-import-sort/imports": "error",
      "simple-import-sort/exports": "error",

      "unicorn/prefer-node-protocol": "error",
      "unicorn/throw-new-error": "error",
      "unicorn/error-message": "error",
      "unicorn/prefer-type-error": "error",
      "unicorn/no-useless-spread": "error",
      "unicorn/no-useless-undefined": "error",
      "unicorn/no-array-reduce": "error",
      "unicorn/no-instanceof-builtins": "error",
      "unicorn/prefer-string-slice": "error",
      "unicorn/prefer-at": "error",
      "unicorn/no-negated-condition": "error",
      "unicorn/prefer-ternary": ["error", "only-single-line"],
      "unicorn/catch-error-name": "error",
      "unicorn/filename-case": "error",

      "@stylistic/max-len": ["error", { code: 100, ignoreUrls: true, ignoreRegExpLiterals: true }],
      "@stylistic/quotes": ["error", "double", { avoidEscape: true }],
      // An operator starts the line it continues, except `=`, which ends the line it opens.
      "@stylistic/operator-linebreak": ["error", "before", { overrides: { "=": "after" } }],
    },
  },
);
