package db

import (
        "fmt"
        "log"
        "os"
        "path/filepath"

        "github.com/newnew/gateway/config"
        "github.com/newnew/gateway/internal/model"
        "golang.org/x/crypto/bcrypt"
        "gorm.io/driver/sqlite"
        "gorm.io/gorm"
        "gorm.io/gorm/logger"
)

var DB *gorm.DB

func Init(cfg *config.Config) error {
        if err := os.MkdirAll(filepath.Dir(cfg.DBPath), 0o755); err != nil {
                return fmt.Errorf("create data dir: %w", err)
        }

        gormCfg := &gorm.Config{
                Logger: logger.Default.LogMode(logger.Warn),
        }
        var err error
        DB, err = gorm.Open(sqlite.Open(cfg.DBPath), gormCfg)
        if err != nil {
                return fmt.Errorf("open db: %w", err)
        }

        if err := DB.AutoMigrate(
                &model.User{},
                &model.Token{},
                &model.Channel{},
                &model.RequestLog{},
                &model.Setting{},
                &model.Conversation{},
                &model.ConversationMessage{},
        ); err != nil {
                return fmt.Errorf("migrate: %w", err)
        }

        if err := seedSettings(); err != nil {
                return err
        }
        return nil
}

func seedAdmin(cfg *config.Config) error {
        var count int64
        if err := DB.Model(&model.User{}).Count(&count).Error; err != nil {
                return err
        }
        if count == 0 {
                hash, err := bcrypt.GenerateFromPassword([]byte(cfg.AdminPassword), bcrypt.DefaultCost)
                if err != nil {
                        return err
                }
                u := model.User{
                        Username:     cfg.AdminUser,
                        PasswordHash: string(hash),
                }
                if err := DB.Create(&u).Error; err != nil {
                        return err
                }
                log.Printf("[seed] admin user created: username=%s password=%s", cfg.AdminUser, cfg.AdminPassword)
                return nil
        }

        // Optional recovery: ADMIN_RESET_PASSWORD=1 resets admin password from env
        if os.Getenv("ADMIN_RESET_PASSWORD") == "1" || os.Getenv("ADMIN_RESET_PASSWORD") == "true" {
                hash, err := bcrypt.GenerateFromPassword([]byte(cfg.AdminPassword), bcrypt.DefaultCost)
                if err != nil {
                        return err
                }
                if err := DB.Model(&model.User{}).
                        Where("username = ?", cfg.AdminUser).
                        Update("password_hash", string(hash)).Error; err != nil {
                        return err
                }
                log.Printf("[seed] admin password reset via ADMIN_RESET_PASSWORD: username=%s password=%s", cfg.AdminUser, cfg.AdminPassword)
        }
        return nil
}

func seedSettings() error {
        defaults := map[string]string{
                model.SettingLogBodyMaxBytes:    "65536",
                model.SettingPriceMissingPolicy: model.PricePolicyAllow,
                model.SettingRequestTimeout:     "300",
        }
        for k, v := range defaults {
                var count int64
                if err := DB.Model(&model.Setting{}).Where("`key` = ?", k).Count(&count).Error; err != nil {
                        return err
                }
                if count == 0 {
                        if err := DB.Create(&model.Setting{Key: k, Value: v}).Error; err != nil {
                                return err
                        }
                }
        }
        return nil
}

func GetSetting(key string) string {
        var s model.Setting
        if err := DB.Where("`key` = ?", key).First(&s).Error; err != nil {
                return ""
        }
        return s.Value
}

func SetSetting(key, value string) error {
        var s model.Setting
        err := DB.Where("`key` = ?", key).First(&s).Error
        if err == gorm.ErrRecordNotFound {
                return DB.Create(&model.Setting{Key: key, Value: value}).Error
        }
        if err != nil {
                return err
        }
        s.Value = value
        return DB.Save(&s).Error
}
