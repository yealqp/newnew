package model

import "time"

type Conversation struct {
	ID        uint      `json:"id" gorm:"primaryKey"`
	Title     string    `json:"title" gorm:"size:256"`
	Model     string    `json:"model" gorm:"size:128"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type ConversationMessage struct {
	ID             uint      `json:"id" gorm:"primaryKey"`
	ConversationID uint      `json:"conversation_id" gorm:"index;not null"`
	Role           string    `json:"role" gorm:"size:32;not null"`
	Content        string    `json:"content" gorm:"type:text"`
	CreatedAt      time.Time `json:"created_at"`
}

func (Conversation) TableName() string {
	return "conversations"
}

func (ConversationMessage) TableName() string {
	return "conversation_messages"
}
