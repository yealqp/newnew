package main

import (
        "log"

        "github.com/gofiber/fiber/v2"
        "github.com/gofiber/fiber/v2/middleware/logger"
        "github.com/gofiber/fiber/v2/middleware/recover"
        "github.com/newnew/gateway/config"
        "github.com/newnew/gateway/internal/db"
        "github.com/newnew/gateway/internal/router"
)

func main() {
        cfg := config.Load()

        if err := db.Init(cfg); err != nil {
                log.Fatalf("db init: %v", err)
        }

        app := fiber.New(fiber.Config{
                BodyLimit:             20 * 1024 * 1024,
                DisableStartupMessage: false,
                StreamRequestBody:     false,
        })
        app.Use(recover.New())
        app.Use(logger.New())

        router.Setup(app)

        addr := ":" + cfg.Port
        log.Printf("gateway listening on %s", addr)
        if err := app.Listen(addr); err != nil {
                log.Fatal(err)
        }
}
