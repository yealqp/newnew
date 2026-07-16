package model

import "time"

type User struct {
        ID           uint      `json:"id" gorm:"primaryKey"`
        Username     string    `json:"username" gorm:"uniqueIndex;size:64;not null"`
        PasswordHash string    `json:"-" gorm:"not null"`
        CreatedAt    time.Time `json:"created_at"`
        UpdatedAt    time.Time `json:"updated_at"`
}
