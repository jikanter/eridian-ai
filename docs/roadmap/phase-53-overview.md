# Phase 53 - Deterministic Injection

So far, the most useful feature I have built for aichat (for me), 
is the ability to wrap pi in bits of functionality. I would like to extend that further by adding  
a discoverability layer into the integration.

In other words, I want to make it easier for users to find functionality of aichat via pi. Because pi 
is so incredibly modular, I want to be able to say:

## Scenario 1 

Read the role <x> and execute <y> with <z>. 
And the pi+aichat chimera will select from roles to find the correct <x>,
the tools to find the correct <y>, and the agents, macros, knowledge, context, and memory to find the correct <z>.

## Scenario 2

I have a local model behaving in a slightly non-deterministic way, so I can inject some type of deterministic 
call into the output, much like a tool call that wraps the output. so I could say 

```jsx
if (check_if_has_the_word_button(<output/>)) {
    return <output/>
}
else { 
    return "couldn't find you the button you were looking for"
}
```
This is somewhat of a contrived example, but I find myself wanting to wrap llm output with a deterministic function
all the time.

## Scenario 3
