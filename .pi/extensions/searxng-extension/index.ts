import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

export default async function (pi: ExtensionAPI) {
  const SEARXNG_BASE_URL = "http://script:3004";

  pi.registerTool({
    name: "searxng_search",
    label: "SearXNG Search",
    description: "Search the web using SearXNG.",
    promptSnippet: "Use this tool to search the web for information.",
    promptGuidelines: [
      "Use searxng_search when you need to find up-to-date information from the web.",
    ],
    parameters: Type.Object({
      query: Type.String({ description: "The search query" }),
    }),
    async execute(toolCallId, params, signal, onUpdate, ctx) {
      onUpdate?.({
        content: [{ type: "text", text: `Searching for: ${params.query}...` }],
      });

      try {
        const url = new URL(`${SEARXNG_BASE_URL}/search`);
        url.searchParams.append("q", encodeURIComponent(params.query));
        url.searchParams.append("format", "json");

        const response = await fetch(url.toString(), { signal });

        if (!response.ok) {
          const errorText = await response.text();
          throw new Error(
            `SearXNG request failed (${response.status}): ${errorText}`,
          );
        }

        const data = await response.json();
        const results = data.results || [];

        if (results.length === 0) {
          return {
            content: [{ type: "text", text: "No results found on SearXNG." }],
            details: { count: 0 },
          };
        }

        // Format the top 10 results for the LLM
        const formattedResults = results
          .slice(0, 10)
          .map((r: any) => {
            const title = r.title || "No title";
            const url = r.url || "No URL";
            const content = r.content || "";
            return `### ${title}\n**URL:** ${url}\n**Snippet:** ${content}\n`;
          })
          .join("\n");

        return {
          content: [
            {
              type: "text",
              text: `Found ${results.length} results. Top 10:\n\n${formattedResults}`,
            },
          ],
          details: { totalResults: results.length },
        };
      } catch (error: any) {
        return {
          content: [
            { type: "text", text: `Error performing search: ${error.message}` },
          ],
          details: { error: error.message },
          isError: true,
        };
      }
    },
  });

  pi.on("session_start", async (event, ctx) => {
    //ctx.ui.notify("SearXNG extension loaded!", "info");
  });
}
