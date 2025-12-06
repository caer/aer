// Simple JavaScript entry point for testing bundling
const greeting = "Hello from bundled JavaScript!";

function greet(name) {
    return `${greeting} Welcome, ${name}!`;
}

export { greet };
export default greeting;
