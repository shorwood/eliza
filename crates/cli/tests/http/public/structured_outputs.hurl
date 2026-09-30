POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "I am sad"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "answer",
      "description": "Deterministic fixture",
      "strict": true,
      "schema": {
        "type": "object",
        "properties": {
          "answer": {"type": "string"},
          "ok": {"type": "boolean"}
        },
        "required": ["answer", "ok"],
        "additionalProperties": false
      }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usage.prompt_tokens" > 3
jsonpath "$.usage.completion_tokens" == 8

POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "status",
      "schema": {
        "type": "object",
        "properties": {"ok": {"type": "boolean"}},
        "required": ["ok"]
      }
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"
body contains "data: [DONE]"

POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "I am sad",
  "text": {
    "format": {
      "type": "json_schema",
      "name": "answer",
      "strict": true,
      "schema": {
        "type": "object",
        "properties": {
          "answer": {"type": "string"},
          "ok": {"type": "boolean"}
        },
        "required": ["answer", "ok"],
        "additionalProperties": false
      }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.output_text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.output[0].content[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usage.input_tokens" > 3

POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "input": "Hello",
  "text": {"format": {"type": "json_object"}}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"response\\\""
body contains "\"type\":\"response.completed\""

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "I am sad"}],
  "output_config": {
    "format": {
      "type": "json_schema",
      "schema": {
        "type": "object",
        "properties": {
          "answer": {"type": "string"},
          "ok": {"type": "boolean"}
        },
        "required": ["answer", "ok"],
        "additionalProperties": false
      }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.content[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usage.input_tokens" > 3

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}],
  "output_config": {
    "format": {
      "type": "json_schema",
      "schema": {
        "type": "object",
        "properties": {"ok": {"type": "boolean"}},
        "required": ["ok"]
      }
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"
body contains "event: message_stop"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "responseFormat": {
      "text": {
        "mimeType": "APPLICATION_JSON",
        "schema": {
          "type": "object",
          "properties": {
            "answer": {"type": "string"},
            "ok": {"type": "boolean"}
          },
          "required": ["answer", "ok"],
          "additionalProperties": false
        }
      }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usageMetadata.promptTokenCount" > 3

POST {{base_url}}/gemini/v1beta/models/eliza-1966:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseFormat": {
      "text": {
        "mimeType": "APPLICATION_JSON",
        "schema": {
          "type": "object",
          "properties": {"ok": {"type": "boolean"}},
          "required": ["ok"]
        }
      }
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"
body contains "\"finishReason\":\"STOP\""

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "responseMimeType": "application/json",
    "responseJsonSchema": {
      "type": "object",
      "properties": {
        "answer": {"type": "string"},
        "ok": {"type": "boolean"}
      },
      "required": ["answer", "ok"],
      "additionalProperties": false
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseMimeType": "application/json",
    "responseJsonSchema": {
      "type": "object",
      "properties": {"ok": {"type": "boolean"}},
      "required": ["ok"]
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"

POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "I am sad"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "answer",
      "schema": {
        "type": "object",
        "properties": {
          "answer": {"type": "string"},
          "ok": {"type": "boolean"}
        },
        "required": ["answer", "ok"],
        "additionalProperties": false
      }
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"

POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {"type": "json_object"}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"response\\\""

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{"role": "user", "content": "I am sad"}],
  "format": {
    "type": "object",
    "properties": {
      "answer": {"type": "string"},
      "ok": {"type": "boolean"}
    },
    "required": ["answer", "ok"],
    "additionalProperties": false
  }
}
HTTP 200
[Asserts]
jsonpath "$.message.content" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.prompt_eval_count" > 3

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "Hello"}],
  "format": "json"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\\\"response\\\""
body contains "\"done\":true"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "responseFormat": {"text": {"mimeType": "TEXT_PLAIN"}}
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {"responseMimeType": "text/plain"}
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"

POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "bad",
      "schema": {"type": "string", "pattern": "x"}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_response_schema"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "response_format"
body contains "/pattern"

POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {"type": "yaml"}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_response_format"
jsonpath "$.error.param" == "response_format"

POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "Hello",
  "text": {
    "format": {
      "type": "json_schema",
      "name": "bad",
      "schema": {"type": "object", "required": ["missing"]}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::invalid_response_schema"
jsonpath "$.error.param" == "text.format"
body contains "/required"

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "Hello"}],
  "output_config": {
    "format": {
      "type": "json_schema",
      "schema": {"type": "number", "minimum": 1}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::anthropic::unsupported_output_schema"
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "invalid_request_error"
body contains "/minimum"

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "Hello"}],
  "output_config": {"format": {"type": "yaml"}}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::anthropic::unsupported_output_format"
jsonpath "$.error.type" == "invalid_request_error"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseFormat": {"text": {"mimeType": "TEXT_PLAIN"}},
    "responseMimeType": "application/json"
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::conflicting_response_formats"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseFormat": {"text": {"mimeType": "YAML"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_response_format"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/flite:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "responseMimeType": "application/json",
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::text_format_for_audio"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{"role": "user", "content": "Hello"}],
  "format": {"$ref": "#"}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::invalid_response_schema"
body contains "/$ref"

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{"role": "user", "content": "Hello"}],
  "format": "yaml"
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::unsupported_response_format"
