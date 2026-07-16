package middleware

import (
        "strings"
        "time"

        "github.com/gofiber/fiber/v2"
        "github.com/golang-jwt/jwt/v5"
        "github.com/google/uuid"
        "github.com/newnew/gateway/config"
        "github.com/newnew/gateway/internal/db"
        "github.com/newnew/gateway/internal/model"
)

const (
        LocalsRequestID = "request_id"
        LocalsUserID    = "user_id"
        LocalsUsername  = "username"
        LocalsToken     = "api_token"
)

func RequestID() fiber.Handler {
        return func(c *fiber.Ctx) error {
                rid := c.Get("X-Request-Id")
                if rid == "" {
                        rid = uuid.NewString()
                }
                c.Locals(LocalsRequestID, rid)
                c.Set("X-Request-Id", rid)
                return c.Next()
        }
}

func CORS() fiber.Handler {
        return func(c *fiber.Ctx) error {
                c.Set("Access-Control-Allow-Origin", "*")
                c.Set("Access-Control-Allow-Methods", "GET,POST,PUT,PATCH,DELETE,OPTIONS")
                c.Set("Access-Control-Allow-Headers", "Origin,Content-Type,Accept,Authorization,x-api-key,anthropic-version,anthropic-beta")
                c.Set("Access-Control-Expose-Headers", "X-Request-Id")
                if c.Method() == fiber.MethodOptions {
                        return c.SendStatus(fiber.StatusNoContent)
                }
                return c.Next()
        }
}

type JWTClaims struct {
        UserID   uint   `json:"user_id"`
        Username string `json:"username"`
        jwt.RegisteredClaims
}

func GenerateJWT(userID uint, username string) (string, error) {
        cfg := config.Get()
        claims := JWTClaims{
                UserID:   userID,
                Username: username,
                RegisteredClaims: jwt.RegisteredClaims{
                        ExpiresAt: jwt.NewNumericDate(time.Now().Add(72 * time.Hour)),
                        IssuedAt:  jwt.NewNumericDate(time.Now()),
                },
        }
        t := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
        return t.SignedString([]byte(cfg.JWTSecret))
}

func AdminAuth() fiber.Handler {
        return func(c *fiber.Ctx) error {
                // never guard login even if mis-mounted
                if c.Method() == fiber.MethodPost && (c.Path() == "/api/admin/login" || strings.HasSuffix(c.Path(), "/login")) {
                        return c.Next()
                }
                auth := c.Get("Authorization")
                if auth == "" || !strings.HasPrefix(auth, "Bearer ") {
                        return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
                                "success": false,
                                "message": "未登录或 token 缺失",
                                "error":   "unauthorized",
                        })
                }
                tokenStr := strings.TrimPrefix(auth, "Bearer ")
                claims := &JWTClaims{}
                token, err := jwt.ParseWithClaims(tokenStr, claims, func(t *jwt.Token) (interface{}, error) {
                        return []byte(config.Get().JWTSecret), nil
                })
                if err != nil || !token.Valid {
                        return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
                                "success": false,
                                "message": "token 无效或已过期，请重新登录",
                                "error":   "invalid token",
                        })
                }
                c.Locals(LocalsUserID, claims.UserID)
                c.Locals(LocalsUsername, claims.Username)
                return c.Next()
        }
}

// TokenAuth validates client API keys (sk-xxx). No quota checks.
func TokenAuth() fiber.Handler {
        return func(c *fiber.Ctx) error {
                key := extractAPIKey(c)
                if key == "" {
                        return c.Status(fiber.StatusUnauthorized).JSON(openAIError("missing api key", "invalid_request_error"))
                }
                var tok model.Token
                if err := db.DB.Where("`key` = ?", key).First(&tok).Error; err != nil {
                        return c.Status(fiber.StatusUnauthorized).JSON(openAIError("invalid api key", "invalid_api_key"))
                }
                if tok.Status != model.TokenStatusEnabled {
                        return c.Status(fiber.StatusUnauthorized).JSON(openAIError("api key disabled", "invalid_api_key"))
                }
                if tok.IsExpired() {
                        return c.Status(fiber.StatusUnauthorized).JSON(openAIError("api key expired", "invalid_api_key"))
                }
                now := time.Now()
                _ = db.DB.Model(&tok).Update("accessed_at", now)
                c.Locals(LocalsToken, &tok)
                return c.Next()
        }
}

func extractAPIKey(c *fiber.Ctx) string {
        if k := c.Get("x-api-key"); k != "" {
                return k
        }
        auth := c.Get("Authorization")
        if strings.HasPrefix(auth, "Bearer ") {
                return strings.TrimPrefix(auth, "Bearer ")
        }
        if auth != "" {
                return auth
        }
        if k := c.Query("key"); k != "" {
                return k
        }
        return ""
}

func openAIError(msg, typ string) fiber.Map {
        return fiber.Map{
                "error": fiber.Map{
                        "message": msg,
                        "type":    typ,
                },
        }
}

func GetToken(c *fiber.Ctx) *model.Token {
        v := c.Locals(LocalsToken)
        if v == nil {
                return nil
        }
        t, _ := v.(*model.Token)
        return t
}

func GetRequestID(c *fiber.Ctx) string {
        v, _ := c.Locals(LocalsRequestID).(string)
        return v
}
