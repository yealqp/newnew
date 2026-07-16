package convert

import (
        "testing"

        "github.com/newnew/gateway/internal/dto"
)

func TestOpenAIChatToClaudeSystem(t *testing.T) {
        max := 100
        req := &dto.OpenAIChatRequest{
                Model: "gpt-4o",
                Messages: []dto.OpenAIMessage{
                        {Role: "system", Content: "you are helpful"},
                        {Role: "user", Content: "hi"},
                },
                MaxTokens: &max,
        }
        out, err := OpenAIChatToClaude(req)
        if err != nil {
                t.Fatal(err)
        }
        if out.System != "you are helpful" {
                t.Fatalf("system: %v", out.System)
        }
        if len(out.Messages) != 1 || out.Messages[0].Role != "user" {
                t.Fatalf("messages: %+v", out.Messages)
        }
        if out.MaxTokens != 100 {
                t.Fatalf("max_tokens: %d", out.MaxTokens)
        }
}

func TestClaudeToOpenAIChat(t *testing.T) {
        req := &dto.ClaudeRequest{
                Model:     "claude-3",
                MaxTokens: 256,
                System:    "sys",
                Messages: []dto.ClaudeMessage{
                        {Role: "user", Content: "hello"},
                },
        }
        out, err := ClaudeToOpenAIChat(req)
        if err != nil {
                t.Fatal(err)
        }
        if len(out.Messages) < 2 {
                t.Fatalf("expected system+user, got %+v", out.Messages)
        }
        if out.Messages[0].Role != "system" || out.Messages[0].Content != "sys" {
                t.Fatalf("system msg: %+v", out.Messages[0])
        }
}

func TestClaudeResponseToOpenAI(t *testing.T) {
        resp := &dto.ClaudeResponse{
                ID:   "msg_1",
                Role: "assistant",
                Content: []dto.ClaudeContentBlock{
                        {Type: "text", Text: "world"},
                },
                Model:      "claude-3",
                StopReason: "end_turn",
                Usage: dto.ClaudeUsage{
                        InputTokens:  10,
                        OutputTokens: 5,
                },
        }
        oai := ClaudeResponseToOpenAI(resp, "claude-3")
        if oai == nil || len(oai.Choices) != 1 {
                t.Fatal("nil response")
        }
        if oai.Choices[0].Message.Content != "world" {
                t.Fatalf("content: %v", oai.Choices[0].Message.Content)
        }
        if oai.Usage == nil || oai.Usage.PromptTokens != 10 || oai.Usage.CompletionTokens != 5 {
                t.Fatalf("usage: %+v", oai.Usage)
        }
}
