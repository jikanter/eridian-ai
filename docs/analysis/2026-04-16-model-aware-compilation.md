# Idea: Model Aware Compilation

After implementating phase 9D, I had the idea of implementing model aware compilation. In other words, 
we would maintain certain information about the model and harness, and give hints to the preflight compiler
to better craft the schema. For example, has a tool calling convention of '<function_call arg1="foo" arg2="bar"/>',
we could help the tool both generate the schema and/or check the schema for consistency with the model. 

My gut tells me we would need to at least maintain a cache of the model cards to save round-trips to the huggingface 
api. And then those model cards would necessarily require some kind of compilation step to make them usable for this 
purpose.