// Simple JavaScript entry point for testing bundling
import { formatMessage, HELPER_VERSION } from './helper.js';

const greeting = "Hello from bundled JavaScript!";

function greet(name) {
    return formatMessage("v" + HELPER_VERSION, `${greeting} Welcome, ${name}!`);
}

export { greet };
export default greeting;
