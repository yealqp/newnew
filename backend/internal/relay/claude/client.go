package claude

import (
        "bufio"
        "bytes"
        "context"
        "encoding/json"
        "fmt"
        "io"
        "net/http"
        "strings"
        "time"
)

type Client struct {
        HTTPClient *http.Client
}

func New(timeoutSec int) *Client {
        if timeoutSec <= 0 {
                timeoutSec = 300
        }
        return &Client{
                HTTPClient: &http.Client{Timeout: time.Duration(timeoutSec) * time.Second},
        }
}

// Messages posts to upstream Claude-compatible messages endpoint.
// If fullURL is true, baseURL is used as-is (complete endpoint).
// Otherwise baseURL is treated as origin and /v1/messages is appended.
func (c *Client) Messages(ctx context.Context, baseURL, apiKey string, body []byte, stream bool, fullURL bool) (*http.Response, error) {
        url := resolveURL(baseURL, fullURL, "/v1/messages")
        req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
        if err != nil {
                return nil, err
        }
        req.Header.Set("Content-Type", "application/json")
        req.Header.Set("x-api-key", apiKey)
        req.Header.Set("anthropic-version", "2023-06-01")
        if stream {
                req.Header.Set("Accept", "text/event-stream")
        }
        return c.HTTPClient.Do(req)
}

func resolveURL(baseURL string, fullURL bool, defaultPath string) string {
        base := strings.TrimSpace(baseURL)
        if fullURL {
                return strings.TrimRight(base, "/")
        }
        return strings.TrimRight(base, "/") + defaultPath
}

func ParseSSE(r io.Reader, onEvent func(event, data string) error) error {
        sc := bufio.NewScanner(r)
        buf := make([]byte, 0, 64*1024)
        sc.Buffer(buf, 10*1024*1024)
        var event string
        for sc.Scan() {
                line := sc.Text()
                if line == "" {
                        event = ""
                        continue
                }
                if strings.HasPrefix(line, "event:") {
                        event = strings.TrimSpace(strings.TrimPrefix(line, "event:"))
                        continue
                }
                if strings.HasPrefix(line, "data:") {
                        data := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
                        if err := onEvent(event, data); err != nil {
                                return err
                        }
                }
        }
        return sc.Err()
}

func EncodeSSE(event, data string) []byte {
        if event == "" {
                return []byte("data: " + data + "\n\n")
        }
        return []byte("event: " + event + "\ndata: " + data + "\n\n")
}

func PrettyError(body []byte) string {
        var m map[string]any
        if err := json.Unmarshal(body, &m); err == nil {
                if e, ok := m["error"].(map[string]any); ok {
                        if msg, ok := e["message"].(string); ok {
                                return msg
                        }
                }
        }
        s := string(body)
        if len(s) > 500 {
                return s[:500]
        }
        return s
}

func JSONError(status int, msg string) error {
        return fmt.Errorf("upstream %d: %s", status, msg)
}
