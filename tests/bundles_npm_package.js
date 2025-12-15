import { Editor } from '@tiptap/core'

new Editor({
  element: document.querySelector('.element'),
  extensions: [],
  content: '<p>Hello World!</p>',
})