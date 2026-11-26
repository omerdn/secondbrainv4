import { isAllowedEmbed, previewNode, safeImage, safeLink } from "@/lib/editor/embeds";

describe("page embeds (Y4, ADR-0013)", () => {
  it("puts only the allowed providers in an iframe", () => {
    expect(isAllowedEmbed("https://www.youtube-nocookie.com/embed/abc")).toBe(true);
    expect(isAllowedEmbed("https://open.spotify.com/embed/track/abc")).toBe(true);
    expect(isAllowedEmbed("https://w.soundcloud.com/player/?url=x")).toBe(true);
    expect(isAllowedEmbed("https://drive.google.com/file/d/abc/preview")).toBe(true);
    for (const bad of [
      "https://evil.example/embed",
      "https://www.youtube-nocookie.com.evil.example/embed/abc",
      "http://www.youtube-nocookie.com/embed/abc",
      "javascript:alert(1)",
      "data:text/html,hi",
      null,
      42,
    ]) {
      expect(isAllowedEmbed(bad)).toBe(false);
    }
  });

  it("links and images are http(s) only", () => {
    expect(safeLink("https://example.com/a")).toBe("https://example.com/a");
    expect(safeLink("javascript:alert(1)")).toBeNull();
    expect(safeLink("not a link")).toBeNull();
    expect(safeImage("https://example.com/i.png")).toBe("https://example.com/i.png");
    expect(safeImage("http://example.com/i.png")).toBeNull();
  });

  it("turns a server preview into an editor node", () => {
    expect(
      previewNode({
        type: "embed",
        url: "https://youtu.be/abc",
        provider: "youtube",
        embed_url: "https://www.youtube-nocookie.com/embed/abc",
      }),
    ).toEqual({
      type: "embed",
      attrs: {
        src: "https://www.youtube-nocookie.com/embed/abc",
        provider: "youtube",
        url: "https://youtu.be/abc",
      },
    });
    expect(
      previewNode({
        type: "link",
        url: "https://example.com/",
        title: "Örnek",
        description: null,
        image: null,
        site_name: "Example",
      }),
    ).toEqual({
      type: "linkPreview",
      attrs: {
        url: "https://example.com/",
        title: "Örnek",
        description: null,
        image: null,
        site: "Example",
      },
    });
  });
});
