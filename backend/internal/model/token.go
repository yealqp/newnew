package model

import "time"

type Token struct {
        ID          uint       `json:"id" gorm:"primaryKey"`
        Name        string     `json:"name" gorm:"size:128;not null"`
        Key         string     `json:"key" gorm:"uniqueIndex;size:128;not null"`
        Status      int        `json:"status" gorm:"default:1"` // 1=enabled 0=disabled
        ModelLimits string     `json:"model_limits" gorm:"type:text"` // JSON array, empty = no limit
        ExpiredAt   int64      `json:"expired_at" gorm:"default:0"`   // 0 = never
        CreatedAt   time.Time  `json:"created_at"`
        AccessedAt  *time.Time `json:"accessed_at"`
}

const (
        TokenStatusDisabled = 0
        TokenStatusEnabled  = 1
)

func (t *Token) IsExpired() bool {
        if t.ExpiredAt == 0 {
                return false
        }
        return time.Now().Unix() > t.ExpiredAt
}
