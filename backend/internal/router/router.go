package router

import (
        "github.com/gofiber/fiber/v2"
        "github.com/newnew/gateway/internal/handler/admin"
        "github.com/newnew/gateway/internal/handler/relay"
        "github.com/newnew/gateway/internal/middleware"
)

func Setup(app *fiber.App) {
        app.Use(middleware.CORS())
        app.Use(middleware.RequestID())

        app.Get("/health", func(c *fiber.Ctx) error {
                return c.JSON(fiber.Map{"status": "ok"})
        })

        // Admin API — login is public; everything else requires JWT
        app.Post("/api/admin/login", admin.Login)

        auth := app.Group("/api/admin", middleware.AdminAuth())
        auth.Get("/me", admin.Me)
        auth.Post("/change-password", admin.ChangePassword)

        auth.Get("/channels", admin.ListChannels)
        auth.Post("/channels/fetch-models", admin.FetchUpstreamModels)
        auth.Get("/channels/:id", admin.GetChannel)
        auth.Post("/channels", admin.CreateChannel)
        auth.Put("/channels/:id", admin.UpdateChannel)
        auth.Delete("/channels/:id", admin.DeleteChannel)
        auth.Get("/channels/:id/test", admin.TestChannel)

        auth.Get("/tokens", admin.ListTokens)
        auth.Post("/tokens", admin.CreateToken)
        auth.Put("/tokens/:id", admin.UpdateToken)
        auth.Post("/tokens/:id/reset-key", admin.ResetTokenKey)
        auth.Delete("/tokens/:id", admin.DeleteToken)

        auth.Get("/logs", admin.ListLogs)
        auth.Get("/logs/:id", admin.GetLog)

        auth.Get("/dashboard", admin.Dashboard)

        auth.Get("/settings", admin.GetSettings)
        auth.Put("/settings", admin.UpdateSettings)

        // Relay API
        h := relay.NewHandler()
        v1 := app.Group("/v1", middleware.TokenAuth())
        v1.Get("/models", h.ListModels)
        v1.Post("/chat/completions", h.ChatCompletions)
        v1.Post("/messages", h.Messages)
}
