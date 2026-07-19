import { readFile, rm, writeFile } from "node:fs/promises";

await rm(new URL("./pkg/.gitignore", import.meta.url), { force: true });

const packageJsonUrl = new URL("./pkg/package.json", import.meta.url);
const packageJson = JSON.parse(await readFile(packageJsonUrl, "utf8"));
packageJson.type = "module";
await writeFile(packageJsonUrl, `${JSON.stringify(packageJson, null, 2)}\n`);
