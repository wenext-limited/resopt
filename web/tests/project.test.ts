import { test, expect } from "bun:test";
import { selectImages, outputPath, cleanPath } from "../src/project";
import {
  parseOptions,
  detectInput,
  artifact,
  createNativeApi,
} from "../scripts/native-api";
const f = (path: string, text = "") => ({
  path,
  file: new File([text], path.split("/").pop()!),
});
test("project inventory respects nested gitignore and ignored parent directories", async () => {
  const result = await selectImages([
    f("Project/.gitignore", "Build/\n*.jpg\n!keep.jpg"),
    f("Project/a.png"),
    f("Project/a.jpg"),
    f("Project/keep.jpg"),
    f("Project/Build/a.png"),
    f("Project/Build/.gitignore", "!a.png"),
    f("Project/Resources/.gitignore", "*.png\n!keep.png"),
    f("Project/Resources/a.png"),
    f("Project/Resources/keep.png"),
    f("Project/main.swift"),
    f("Project/.git/x.png"),
  ]);
  expect(result.images.map((f) => f.path)).toEqual([
    "Project/a.png",
    "Project/keep.jpg",
    "Project/Resources/keep.png",
  ]);
  expect(result.excluded).toBe(4);
});
test("archive paths preserve folders without traversal or format collisions", () => {
  expect(() => cleanPath("../x.png")).toThrow();
  const used = new Set<string>();
  expect(outputPath("Assets/a.png", "heic", used)).toBe("Assets/a.heic");
  expect(outputPath("Assets/a.jpg", "heic", used)).toBe("Assets/a-2.heic");
});
test("native API validates options, bytes, paths and same-origin requests", async () => {
  expect(() =>
    parseOptions(new URL("http://local/?format=jpeg&quality=85")),
  ).not.toThrow();
  expect(() =>
    parseOptions(new URL("http://local/?format=sh&quality=1")),
  ).toThrow();
  expect(() =>
    detectInput(new TextEncoder().encode("not an image file")),
  ).toThrow();
  expect(artifact("/tmp/output", "../secret")).rejects.toThrow();
  const api = createNativeApi("/missing", "http://localhost:1234");
  expect(
    (
      await api(
        new Request("http://localhost:1234/api/convert", { method: "POST" }),
      )
    ).status,
  ).toBe(403);
  expect(
    (await api(new Request("http://localhost:1234/api/convert"))).status,
  ).toBe(405);
});

test("removing ignore rules restores excluded image candidates", async () => {
  const entries = [
    f("P/.gitignore", "Build/"),
    f("P/Build/a.png"),
    f("P/a.png"),
  ];
  expect((await selectImages(entries)).images.length).toBe(1);
  expect(
    (await selectImages(entries.filter((x) => !x.path.endsWith(".gitignore"))))
      .images.length,
  ).toBe(2);
});
