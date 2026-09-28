import { fixupConfigRules } from "@eslint/compat";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTypescript from "eslint-config-next/typescript";

const config = [
  // eslint-plugin-react, jsx-a11y and import (via eslint-config-next) still
  // call context methods ESLint 10 removed; the official shim restores them.
  ...fixupConfigRules([...nextVitals, ...nextTypescript]),
  {
    ignores: [".next/**", "node_modules/**", "playwright-report/**", "test-results/**", "next-env.d.ts", "src/lib/api-types.ts"],
  },
  {
    rules: {
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
    },
  },
];

export default config;
