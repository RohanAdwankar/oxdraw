One example of a way to get LLM Access for free is Google AI Studio.
Here is the steps to set it up:
1. Go to [Google AI Studio](https://aistudio.google.com/api-keys) and sign in with your Google account.
2. Click on "Create API Key" and "Create Project" and copy the generated API key (the names don't matter).
3. Paste the API key into the `oxdraw --code-map ./ --gemini YOUR_API_KEY` flag when running the code map generation command.
This process is fairly straightforward and doesn't require any billing information.
The other default option (which [I used](https://github.com/RohanAdwankar/cgpu?tab=readme-ov-file#serve-gemini-for-free-as-openai-compatible-api) during development) is to serve local inference on this url http://localhost:8080/v1/responses
The response format should be compatible with OpenAI's responses API.
