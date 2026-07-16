package config

import (
        "os"
        "strconv"
        "sync"

        "github.com/joho/godotenv"
)

type Config struct {
        Port           string
        DBPath         string
        JWTSecret      string
        AdminUser      string
        AdminPassword  string
        RequestTimeout int // seconds
}

var (
        cfg  *Config
        once sync.Once
)

func Load() *Config {
        once.Do(func() {
                _ = godotenv.Load()
                cfg = &Config{
                        Port:           getEnv("PORT", "3000"),
                        DBPath:         getEnv("DB_PATH", "data/gateway.db"),
                        JWTSecret:      getEnv("JWT_SECRET", "change-me-in-production-please"),
                        AdminUser:      getEnv("ADMIN_USER", "admin"),
                        AdminPassword:  getEnv("ADMIN_PASSWORD", "admin123"),
                        RequestTimeout: getEnvInt("REQUEST_TIMEOUT", 300),
                }
        })
        return cfg
}

func Get() *Config {
        if cfg == nil {
                return Load()
        }
        return cfg
}

func getEnv(key, def string) string {
        if v := os.Getenv(key); v != "" {
                return v
        }
        return def
}

func getEnvInt(key string, def int) int {
        if v := os.Getenv(key); v != "" {
                if n, err := strconv.Atoi(v); err == nil {
                        return n
                }
        }
        return def
}
