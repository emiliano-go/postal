import { appendFileSync, readFileSync } from "node:fs";

const [label, path] = process.argv.slice(2);
if (!label || !path) throw new Error("usage: coverage-summary.mjs LABEL REPORT.lcov");

const totals = { LF: 0, LH: 0, FNF: 0, FNH: 0, BRF: 0, BRH: 0 };
let files = 0;
for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
  if (line.startsWith("SF:")) files++;
  const separator = line.indexOf(":");
  const field = line.slice(0, separator);
  if (Object.hasOwn(totals, field)) totals[field] += Number(line.slice(separator + 1));
}
if (!files || !totals.LF) throw new Error(`empty coverage report: ${path}`);

const percent = (covered, total) => total ? `${(100 * covered / total).toFixed(1)}% (${covered}/${total})` : "n/a";
const summary = [
  `### ${label} coverage`,
  "| Lines | Functions | Branches | Files |",
  "| ---: | ---: | ---: | ---: |",
  `| ${percent(totals.LH, totals.LF)} | ${percent(totals.FNH, totals.FNF)} | ${percent(totals.BRH, totals.BRF)} | ${files} |`,
  "",
].join("\n");
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
process.stdout.write(summary);
