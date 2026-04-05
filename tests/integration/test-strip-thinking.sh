#!/usr/bin/env bats
@test "query aichat and strip thinking" {
  result=$(echo "How are you today" |aichat strip-thinking|grep "<think>")
  [  "$result" = "" ]
}
