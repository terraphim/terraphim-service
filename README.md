# terraphim-service

Service, middleware, routing, haystack providers, session analysis and the agent
spawner for Terraphim AI -- extracted from the `terraphim-ai` monorepo (Gitea
#1910). session-analyzer and spawner live here because middleware/router depend
on them (layering correction). Depends on terraphim-core and
terraphim-config-persistence via the Terraphim org registry.
