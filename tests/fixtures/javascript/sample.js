'use strict'
// Helper
const path = require('path')
import fs from 'fs'

/**
 * Leverages the utility to seamlessly compute values.
 */
function computeTotalValue(itemArray, { multiplier = 2, ...options } = {}) {
  let runningTotal = 0
  for (const currentItem of itemArray) {
    runningTotal += currentItem * multiplier // accumulate
  }
  const finalize = function namedInner(value) { return value }
  return runningTotal
}

class ShoppingCart extends Base {
  items = []
  #hiddenTotal = 0
  static VERSION = '1.0.0'

  constructor(ownerName) { super(); this.ownerName = ownerName }
  addItem(newItem) { this.items.push(newItem) }
  get total() { return this.#hiddenTotal }
}

const messages = { greeting: 'Welcome back to the shop', 'multi word key': 1 }
const arrow = (a, b) => a + b
try { } catch ({ message }) { console.error(`Something went wrong: ${message}`) }
